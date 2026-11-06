//! Switch planning and execution.

use crate::config::Vpn;
use crate::runner::{Runner, describe_exit};
use crate::state::VpnState;

/// One command to run, by VPN index in the configuration.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Action {
    /// Stop a VPN; failure aborts the switch.
    Down(usize),
    /// Stop a VPN whose state is unknown; failure only warns, since it was
    /// probably down already.
    EnsureDown(usize),
    /// Start a VPN; failure aborts the switch.
    Up(usize),
}

impl Action {
    pub fn index(self) -> usize {
        match self {
            Action::Down(i) | Action::EnsureDown(i) | Action::Up(i) => i,
        }
    }

    pub fn verb(self) -> &'static str {
        match self {
            Action::Down(_) | Action::EnsureDown(_) => "down",
            Action::Up(_) => "up",
        }
    }
}

/// Progress notifications emitted by [`execute`].
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum Step<'a> {
    Starting(Action, &'a str),
    Done(Action, &'a str),
    /// An [`Action::EnsureDown`] failed; the switch goes on. Carries how it ended.
    Ignored(Action, &'a str, String),
}

#[derive(thiserror::Error, Debug)]
pub enum SwitchError {
    #[error("`{name}` {verb} command failed ({exit})")]
    CommandFailed {
        name: String,
        verb: &'static str,
        exit: String,
    },
    #[error("cannot start the `{name}` {verb} command")]
    Spawn {
        name: String,
        verb: &'static str,
        source: std::io::Error,
    },
    #[error("no VPN at index {0}")]
    NoSuchVpn(usize),
}

/// Actions that leave only `target` up (`None`: everything down).
///
/// Every other VPN that is up, or whose state is unknown, is brought down first,
/// in configuration order; the target is brought up last unless it already is.
pub fn plan_switch(states: &[VpnState], target: Option<usize>) -> Vec<Action> {
    let downs = states
        .iter()
        .enumerate()
        .filter(|&(i, _)| Some(i) != target)
        .filter_map(|(i, &state)| match state {
            VpnState::Up => Some(Action::Down(i)),
            VpnState::Unknown => Some(Action::EnsureDown(i)),
            VpnState::Down => None,
        });
    let up = target
        .filter(|&t| states.get(t) != Some(&VpnState::Up))
        .map(Action::Up);
    downs.chain(up).collect()
}

/// Actions that bring `index` down (nothing if it is known to be down).
pub fn plan_down(states: &[VpnState], index: usize) -> Vec<Action> {
    if states.get(index) == Some(&VpnState::Down) {
        Vec::new()
    } else {
        vec![Action::Down(index)]
    }
}

/// Runs `actions` in order, stopping at the first failure (a failed
/// [`Action::EnsureDown`] is only reported, as [`Step::Ignored`]).
pub fn execute(
    actions: &[Action],
    vpns: &[Vpn],
    runner: &dyn Runner,
    mut on_step: impl FnMut(Step),
) -> Result<(), SwitchError> {
    for &action in actions {
        let vpn = vpns
            .get(action.index())
            .ok_or(SwitchError::NoSuchVpn(action.index()))?;
        let command = match action {
            Action::Down(_) | Action::EnsureDown(_) => &vpn.down,
            Action::Up(_) => &vpn.up,
        };
        on_step(Step::Starting(action, &vpn.name));
        let status = runner.run(command).map_err(|source| SwitchError::Spawn {
            name: vpn.name.clone(),
            verb: action.verb(),
            source,
        })?;
        if status.success() {
            on_step(Step::Done(action, &vpn.name));
        } else if let Action::EnsureDown(_) = action {
            on_step(Step::Ignored(action, &vpn.name, describe_exit(status)));
        } else {
            return Err(SwitchError::CommandFailed {
                name: vpn.name.clone(),
                verb: action.verb(),
                exit: describe_exit(status),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::runner::Probe;
    use VpnState::{Down, Unknown, Up};
    use std::os::unix::process::ExitStatusExt;
    use std::process::ExitStatus;
    use std::sync::Mutex;
    use std::time::Duration;

    #[test]
    fn switch_downs_active_then_ups_target() {
        assert_eq!(
            plan_switch(&[Up, Down, Down], Some(2)),
            [Action::Down(0), Action::Up(2)]
        );
    }

    #[test]
    fn switch_downs_unknown_too() {
        assert_eq!(
            plan_switch(&[Unknown, Down, Up], Some(1)),
            [Action::EnsureDown(0), Action::Down(2), Action::Up(1)]
        );
    }

    #[test]
    fn target_already_up_only_cleans_others() {
        assert_eq!(plan_switch(&[Up, Up], Some(0)), [Action::Down(1)]);
        assert_eq!(plan_switch(&[Up, Down], Some(0)), []);
    }

    #[test]
    fn unknown_target_is_brought_up() {
        assert_eq!(plan_switch(&[Unknown, Down], Some(0)), [Action::Up(0)]);
    }

    #[test]
    fn all_off() {
        assert_eq!(
            plan_switch(&[Up, Down, Unknown], None),
            [Action::Down(0), Action::EnsureDown(2)]
        );
    }

    #[test]
    fn plan_down_skips_down_vpn() {
        assert_eq!(plan_down(&[Up, Down, Unknown], 0), [Action::Down(0)]);
        assert_eq!(plan_down(&[Up, Down, Unknown], 1), []);
        assert_eq!(plan_down(&[Up, Down, Unknown], 2), [Action::Down(2)]);
    }

    /// Records commands; any command containing "fail" returns false.
    #[derive(Default)]
    struct Recorder(Mutex<Vec<String>>);

    impl Runner for Recorder {
        fn probe(&self, _cmd: &str, _timeout: Duration) -> Probe {
            unreachable!("execute must not probe")
        }

        fn run(&self, cmd: &str) -> std::io::Result<ExitStatus> {
            self.0.lock().unwrap().push(cmd.to_string());
            let code = if cmd.contains("fail") { 1 } else { 0 };
            Ok(ExitStatus::from_raw(code << 8))
        }
    }

    fn two_vpns(a_down: &str) -> Config {
        Config::from_toml(&format!(
            r#"
            [[vpn]]
            name = "a"
            up = "a-up"
            down = "{a_down}"
            [[vpn]]
            name = "b"
            up = "b-up"
            down = "b-down"
            "#
        ))
        .unwrap()
    }

    #[test]
    fn execute_stops_at_first_failure() {
        let config = two_vpns("a-fail");
        let runner = Recorder::default();
        let err = execute(
            &[Action::Down(0), Action::Up(1)],
            &config.vpns,
            &runner,
            |_| {},
        )
        .unwrap_err();
        assert!(matches!(
            err,
            SwitchError::CommandFailed { ref name, verb: "down", .. } if name == "a"
        ));
        assert_eq!(err.to_string(), "`a` down command failed (exit code 1)");
        assert_eq!(*runner.0.lock().unwrap(), ["a-fail"]);
    }

    #[test]
    fn failed_ensure_down_warns_and_goes_on() {
        let config = two_vpns("a-fail");
        let runner = Recorder::default();
        let mut steps = Vec::new();
        execute(
            &[Action::EnsureDown(0), Action::Up(1)],
            &config.vpns,
            &runner,
            |step| steps.push(format!("{step:?}")),
        )
        .unwrap();
        assert_eq!(
            steps,
            [
                "Starting(EnsureDown(0), \"a\")",
                "Ignored(EnsureDown(0), \"a\", \"exit code 1\")",
                "Starting(Up(1), \"b\")",
                "Done(Up(1), \"b\")",
            ]
        );
    }

    #[test]
    fn execute_rejects_unknown_index() {
        let config = two_vpns("a-down");
        let err =
            execute(&[Action::Up(5)], &config.vpns, &Recorder::default(), |_| {}).unwrap_err();
        assert!(matches!(err, SwitchError::NoSuchVpn(5)));
    }

    #[test]
    fn execute_emits_steps_in_order() {
        let config = two_vpns("a-down");
        let runner = Recorder::default();
        let mut steps = Vec::new();
        execute(
            &[Action::Down(0), Action::Up(1)],
            &config.vpns,
            &runner,
            |step| steps.push(format!("{step:?}")),
        )
        .unwrap();
        assert_eq!(
            steps,
            [
                "Starting(Down(0), \"a\")",
                "Done(Down(0), \"a\")",
                "Starting(Up(1), \"b\")",
                "Done(Up(1), \"b\")",
            ]
        );
        assert_eq!(*runner.0.lock().unwrap(), ["a-down", "b-up"]);
    }
}
