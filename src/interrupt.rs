//! Interrupt handling: Ctrl-C, SIGTERM and SIGHUP.
//!
//! Outside a switch, a signal ends `vpns` at once. During a switch, the first
//! signal asks it to stop: the running up/down command gets SIGTERM, the
//! remaining steps are skipped, and the resulting state is still shown. A
//! second signal ends `vpns` at once.

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering};

use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;

/// Whether a switch is running.
static SWITCHING: AtomicBool = AtomicBool::new(false);
/// Signals received during the current switch.
static REQUESTS: AtomicUsize = AtomicUsize::new(0);
/// Pid of the running up/down command, 0 when none.
static CHILD: AtomicI32 = AtomicI32::new(0);

/// What the signal handler should do.
#[derive(Debug, PartialEq, Eq)]
pub enum Disposition {
    /// Kill the status probes and exit.
    Exit,
    /// Let the switch wind down.
    Continue,
}

/// Marks the duration of a switch; signals are deferred while it lives.
pub struct SwitchGuard(());

/// Starts deferring signals until the returned guard is dropped.
pub fn begin_switch() -> SwitchGuard {
    REQUESTS.store(0, Ordering::SeqCst);
    SWITCHING.store(true, Ordering::SeqCst);
    SwitchGuard(())
}

impl Drop for SwitchGuard {
    fn drop(&mut self) {
        SWITCHING.store(false, Ordering::SeqCst);
    }
}

/// Whether a signal asked the current switch to stop.
pub fn requested() -> bool {
    REQUESTS.load(Ordering::SeqCst) > 0
}

/// Waits (up to `timeout`) for the signal handler to record a pending signal.
///
/// When a command dies of the terminal's SIGINT, `vpns` got the same signal, but
/// its handler thread may not have run yet. Ending the switch before it does would
/// let that late handler see no switch and exit without a report.
pub fn settle(timeout: std::time::Duration) {
    let deadline = std::time::Instant::now() + timeout;
    while !requested() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

/// Called by the signal handler.
pub fn on_signal() -> Disposition {
    if !SWITCHING.load(Ordering::SeqCst) || REQUESTS.fetch_add(1, Ordering::SeqCst) > 0 {
        return Disposition::Exit;
    }
    // On Ctrl-C the terminal already sent SIGINT to the command; SIGTERM, SIGHUP
    // or a signal aimed at `vpns` alone did not reach it.
    let pid = CHILD.load(Ordering::SeqCst);
    if pid > 0 {
        let _ = kill(Pid::from_raw(pid), Signal::SIGTERM);
    }
    Disposition::Continue
}

/// Keeps `pid` as the running up/down command while alive.
pub(crate) struct ChildGuard(i32);

pub(crate) fn track_child(pid: u32) -> ChildGuard {
    let pid = i32::try_from(pid).unwrap_or_default();
    if pid > 0 {
        CHILD.store(pid, Ordering::SeqCst);
    }
    ChildGuard(pid)
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = CHILD.compare_exchange(self.0, 0, Ordering::SeqCst, Ordering::SeqCst);
    }
}

/// Serializes the tests that run interactive commands (the tracked pid is global).
#[cfg(test)]
pub(crate) fn test_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::{Runner, ShellRunner};
    use std::time::{Duration, Instant};

    // One test: the state is process-global.
    #[test]
    fn signal_flow() {
        let _lock = test_lock();
        // Outside a switch: exit.
        assert_eq!(on_signal(), Disposition::Exit);
        assert!(!requested());

        // During a switch: the first signal stops the running command and defers.
        let guard = begin_switch();
        let handle = std::thread::spawn(|| ShellRunner.run("exec sleep 5"));
        std::thread::sleep(Duration::from_millis(300));
        let start = Instant::now();
        assert_eq!(on_signal(), Disposition::Continue);
        let status = handle.join().unwrap().unwrap();
        assert!(start.elapsed() < Duration::from_secs(2));
        assert!(!status.success());
        assert!(requested());

        // A second signal exits.
        assert_eq!(on_signal(), Disposition::Exit);

        // A new switch starts clean; after it, signals exit again.
        drop(guard);
        let guard = begin_switch();
        assert!(!requested());
        drop(guard);
        assert_eq!(on_signal(), Disposition::Exit);
    }
}
