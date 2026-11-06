//! Shell command execution.

use std::io::Read;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::thread::{self, JoinHandle};
use std::time::Duration;

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
    /// Runs `cmd` attached to the terminal; returns whether it succeeded.
    fn run(&self, cmd: &str) -> std::io::Result<bool>;
}

/// Runs commands through `sh -c`.
pub struct ShellRunner;

impl Runner for ShellRunner {
    fn probe(&self, cmd: &str, timeout: Duration) -> Probe {
        // Own process group, so a timeout kills the whole pipeline, not just `sh`.
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
        // Drain both pipes concurrently so a chatty command cannot block on a full pipe.
        let stdout = drain(child.stdout.take());
        let stderr = drain(child.stderr.take());

        match child.wait_timeout(timeout) {
            Ok(Some(status)) => {
                let mut output = stdout.join().unwrap_or_default();
                output.push_str(&stderr.join().unwrap_or_default());
                Probe::Exited {
                    code: status.code(),
                    output,
                }
            }
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

    fn run(&self, cmd: &str) -> std::io::Result<bool> {
        Ok(Command::new("sh").arg("-c").arg(cmd).status()?.success())
    }
}

fn drain<R: Read + Send + 'static>(pipe: Option<R>) -> JoinHandle<String> {
    thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut pipe) = pipe {
            // A read error just truncates the output; the exit code still counts.
            let _ = pipe.read_to_end(&mut buf);
        }
        String::from_utf8_lossy(&buf).into_owned()
    })
}

/// Kills the child's process group and reaps the child. Reader threads are left
/// to finish on their own: they end as soon as the killed processes close the pipes.
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
    fn run_reports_success() {
        assert!(ShellRunner.run("true").unwrap());
        assert!(!ShellRunner.run("false").unwrap());
    }
}
