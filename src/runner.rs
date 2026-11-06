//! Shell command execution.

use std::io::Read;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use nix::sys::signal::{Signal, killpg};
use nix::unistd::Pid;
use wait_timeout::ChildExt;

/// How long a status command may run before its VPN is reported as unknown.
pub const STATUS_TIMEOUT: Duration = Duration::from_secs(10);

/// Result of a non-interactive status probe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Probe {
    /// The command exited; `output` is stdout followed by stderr.
    Exited { code: Option<i32>, output: String },
    /// The command did not finish in time and was killed.
    TimedOut,
    /// The command could not be started.
    Failed(String),
}

/// Executes shell commands; faked in tests.
pub trait Runner: Sync {
    /// Runs `cmd` without a terminal and captures its output.
    fn probe(&self, cmd: &str, timeout: Duration) -> Probe;
    /// Runs `cmd` attached to the terminal.
    fn run(&self, cmd: &str) -> std::io::Result<ExitStatus>;
}

/// Runs commands through `sh -c`.
pub struct ShellRunner;

impl Runner for ShellRunner {
    fn probe(&self, cmd: &str, timeout: Duration) -> Probe {
        let deadline = Instant::now() + timeout;
        // Own process group, so a timeout or Ctrl-C kills the whole pipeline, not just `sh`.
        let spawned = Command::new("sh")
            .arg("-c")
            .arg(cmd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0)
            .spawn();
        let mut child = match spawned {
            Ok(child) => child,
            Err(err) => return Probe::Failed(err.to_string()),
        };
        let _live = LiveGroup::register(&child);

        // Drain both pipes concurrently so a chatty command cannot block on a full pipe.
        let (tx, rx) = mpsc::channel();
        drain(child.stdout.take(), Pipe::Stdout, tx.clone());
        drain(child.stderr.take(), Pipe::Stderr, tx);

        // The deadline covers the pipes too: a background process left holding them
        // must not keep the probe waiting once `sh` has exited.
        let (mut stdout, mut stderr) = (String::new(), String::new());
        for _ in 0..2 {
            match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                Ok((Pipe::Stdout, text)) => stdout = text,
                Ok((Pipe::Stderr, text)) => stderr = text,
                Err(_) => {
                    kill_group(&mut child);
                    return Probe::TimedOut;
                }
            }
        }
        match child.wait_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(Some(status)) => Probe::Exited {
                code: status.code(),
                output: stdout + &stderr,
            },
            Ok(None) => {
                kill_group(&mut child);
                Probe::TimedOut
            }
            Err(err) => {
                kill_group(&mut child);
                Probe::Failed(err.to_string())
            }
        }
    }

    fn run(&self, cmd: &str) -> std::io::Result<ExitStatus> {
        let _interactive = Interactive::start();
        Command::new("sh").arg("-c").arg(cmd).status()
    }
}

/// Human description of how a command ended: `exit code 3`, `interrupted`…
pub fn describe_exit(status: ExitStatus) -> String {
    match (status.code(), status.signal()) {
        (Some(code), _) => format!("exit code {code}"),
        (None, Some(signal)) if signal == Signal::SIGINT as i32 => "interrupted".to_string(),
        (None, Some(signal)) => format!("killed by signal {signal}"),
        (None, None) => "unknown exit status".to_string(),
    }
}

/// Number of interactive up/down commands currently running.
static INTERACTIVE: AtomicUsize = AtomicUsize::new(0);

/// Whether an interactive up/down command currently owns the terminal.
///
/// The Ctrl-C handler leaves those alone: the terminal's SIGINT already reaches
/// them, and `vpns` must outlive them to report what happened.
pub fn interactive_child_running() -> bool {
    INTERACTIVE.load(Ordering::SeqCst) > 0
}

/// Counts as an interactive command while alive.
struct Interactive;

impl Interactive {
    fn start() -> Self {
        INTERACTIVE.fetch_add(1, Ordering::SeqCst);
        Interactive
    }
}

impl Drop for Interactive {
    fn drop(&mut self) {
        INTERACTIVE.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Process groups of the status probes currently running.
static LIVE_GROUPS: Mutex<Vec<i32>> = Mutex::new(Vec::new());

fn live_groups() -> MutexGuard<'static, Vec<i32>> {
    LIVE_GROUPS.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Kills every status probe still running (used on Ctrl-C).
///
/// Probes live in their own process groups, so the terminal's SIGINT never
/// reaches them.
pub fn kill_live_probes() {
    for &pgid in live_groups().iter() {
        let _ = killpg(Pid::from_raw(pgid), Signal::SIGKILL);
    }
}

/// Keeps a probe's process group in [`LIVE_GROUPS`] while it runs.
struct LiveGroup(Option<i32>);

impl LiveGroup {
    fn register(child: &Child) -> Self {
        let pgid = i32::try_from(child.id()).ok();
        if let Some(pgid) = pgid {
            live_groups().push(pgid);
        }
        LiveGroup(pgid)
    }
}

impl Drop for LiveGroup {
    fn drop(&mut self) {
        if let Some(pgid) = self.0 {
            live_groups().retain(|&g| g != pgid);
        }
    }
}

#[derive(Clone, Copy)]
enum Pipe {
    Stdout,
    Stderr,
}

fn drain<R: Read + Send + 'static>(pipe: Option<R>, which: Pipe, tx: Sender<(Pipe, String)>) {
    thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut pipe) = pipe {
            // A read error just truncates the output; the exit code still counts.
            let _ = pipe.read_to_end(&mut buf);
        }
        // The receiver is gone once the probe gave up; nothing left to report.
        let _ = tx.send((which, String::from_utf8_lossy(&buf).into_owned()));
    });
}

/// Kills the child's process group, then reaps the child. The child is not reaped
/// before this point, so its pid (and group id) cannot have been reused.
fn kill_group(child: &mut Child) {
    if let Ok(pid) = i32::try_from(child.id()) {
        let _ = killpg(Pid::from_raw(pid), Signal::SIGKILL);
    }
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn probe_captures_output_and_code() {
        let probe = ShellRunner.probe("echo hi; echo err >&2; exit 3", STATUS_TIMEOUT);
        let Probe::Exited { code, output } = probe else {
            panic!("unexpected probe: {probe:?}");
        };
        assert_eq!(code, Some(3));
        assert!(output.contains("hi"));
        assert!(output.contains("err"));
    }

    #[test]
    fn probe_times_out() {
        let start = Instant::now();
        let probe = ShellRunner.probe("sleep 5", Duration::from_millis(200));
        assert_eq!(probe, Probe::TimedOut);
        assert!(start.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn probe_times_out_with_background_child_holding_pipes() {
        let start = Instant::now();
        let probe = ShellRunner.probe("sleep 5 & wait", Duration::from_millis(200));
        assert_eq!(probe, Probe::TimedOut);
        assert!(start.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn probe_bounded_when_background_child_keeps_pipe() {
        let start = Instant::now();
        let probe = ShellRunner.probe("sleep 5 & exit 0", Duration::from_millis(200));
        assert_eq!(probe, Probe::TimedOut);
        assert!(start.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn kill_live_probes_stops_running_probe() {
        let start = Instant::now();
        let handle = std::thread::spawn(|| ShellRunner.probe("sleep 5", STATUS_TIMEOUT));
        std::thread::sleep(Duration::from_millis(300));
        kill_live_probes();
        let probe = handle.join().unwrap();
        assert!(start.elapsed() < Duration::from_secs(2), "{probe:?}");
    }

    #[test]
    fn run_reports_exit_status() {
        assert!(ShellRunner.run("true").unwrap().success());
        assert_eq!(ShellRunner.run("exit 4").unwrap().code(), Some(4));
    }

    #[test]
    fn interactive_flag_is_set_while_a_command_runs() {
        let handle = std::thread::spawn(|| ShellRunner.run("sleep 0.5"));
        std::thread::sleep(Duration::from_millis(200));
        assert!(interactive_child_running());
        handle.join().unwrap().unwrap();
    }

    #[test]
    fn describe_exit_texts() {
        use std::os::unix::process::ExitStatusExt;
        assert_eq!(describe_exit(ExitStatus::from_raw(3 << 8)), "exit code 3");
        assert_eq!(describe_exit(ExitStatus::from_raw(2)), "interrupted");
        assert_eq!(describe_exit(ExitStatus::from_raw(9)), "killed by signal 9");
    }
}
