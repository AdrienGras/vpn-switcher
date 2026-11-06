//! VPN state detection.

use crate::config::Vpn;
use crate::runner::{Probe, Runner, STATUS_TIMEOUT};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VpnState {
    Up,
    Down,
    Unknown,
}

/// A VPN's state and, for `--verbose`, why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Observation {
    pub state: VpnState,
    pub detail: String,
}

/// Interprets the result of a VPN's status command, with an explanation.
pub fn observe(vpn: &Vpn, probe: &Probe) -> Observation {
    use VpnState::{Down, Unknown, Up};
    let seen = |state, detail: String| Observation { state, detail };

    if vpn.status.is_none() {
        return seen(Unknown, "no status command".to_string());
    }
    let (code, output) = match probe {
        Probe::Exited { code, output } => (*code, output),
        Probe::TimedOut => {
            let secs = STATUS_TIMEOUT.as_secs();
            return seen(Unknown, format!("status command timed out after {secs}s"));
        }
        Probe::Failed(err) => return seen(Unknown, format!("cannot run status command: {err}")),
    };
    let Some(code) = code else {
        return seen(Unknown, "status command was killed".to_string());
    };
    if let Some(pattern) = &vpn.up_pattern {
        // Not executable or not found: the output proves nothing.
        if code == 126 || code == 127 {
            return seen(
                Unknown,
                format!("status command not found (exit code {code})"),
            );
        }
        return if pattern.is_match(output) {
            seen(Up, format!("output matches `{pattern}`"))
        } else {
            seen(Down, format!("output does not match `{pattern}`"))
        };
    }
    let state = if vpn.up_codes.contains(&code) {
        Up
    } else if vpn.down_codes.contains(&code) {
        Down
    } else {
        Unknown
    };
    seen(state, format!("exit code {code}"))
}

/// The bare states, in the same order.
pub fn states(observations: &[Observation]) -> Vec<VpnState> {
    observations.iter().map(|o| o.state).collect()
}

/// Interprets the result of a VPN's status command.
pub fn classify(vpn: &Vpn, probe: &Probe) -> VpnState {
    observe(vpn, probe).state
}

/// Probes every VPN in parallel; results follow the configuration order.
pub fn probe_all(vpns: &[Vpn], runner: &dyn Runner) -> Vec<Observation> {
    std::thread::scope(|scope| {
        let handles: Vec<_> = vpns
            .iter()
            .map(|vpn| {
                scope.spawn(move || match &vpn.status {
                    Some(cmd) => observe(vpn, &runner.probe(cmd, STATUS_TIMEOUT)),
                    None => observe(vpn, &Probe::Failed(String::new())),
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| {
                handle.join().unwrap_or_else(|_| Observation {
                    state: VpnState::Unknown,
                    detail: "status check crashed".to_string(),
                })
            })
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use std::time::Duration;

    fn vpn(extra: &str) -> Vpn {
        let toml = format!("[[vpn]]\nname = \"a\"\nup = \"u\"\ndown = \"d\"\n{extra}\n");
        Config::from_toml(&toml).unwrap().vpns.remove(0)
    }

    fn exited(code: i32, output: &str) -> Probe {
        Probe::Exited {
            code: Some(code),
            output: output.to_string(),
        }
    }

    #[test]
    fn pattern_wins_over_code() {
        let v = vpn("status = \"s\"\nup_pattern = \"Status: Connected\"");
        assert_eq!(
            classify(&v, &exited(1, "Status: Connected\n")),
            VpnState::Up
        );
        assert_eq!(
            classify(&v, &exited(0, "Status: Disconnected\n")),
            VpnState::Down
        );
    }

    #[test]
    fn pattern_with_broken_command_is_unknown() {
        let v = vpn("status = \"s\"\nup_pattern = \"Connected\"");
        assert_eq!(
            classify(&v, &exited(127, "sh: s: not found")),
            VpnState::Unknown
        );
        assert_eq!(
            classify(&v, &exited(126, "permission denied")),
            VpnState::Unknown
        );
        let crashed = Probe::Exited {
            code: None,
            output: String::new(),
        };
        assert_eq!(classify(&v, &crashed), VpnState::Unknown);
    }

    #[test]
    fn observation_explains_state() {
        let detail = |extra: &str, probe: Probe| observe(&vpn(extra), &probe).detail;
        assert_eq!(detail("", exited(0, "")), "no status command");
        assert_eq!(
            detail("status = \"s\"", Probe::TimedOut),
            "status command timed out after 10s"
        );
        assert_eq!(
            detail("status = \"s\"", Probe::Failed("boom".into())),
            "cannot run status command: boom"
        );
        assert_eq!(detail("status = \"s\"", exited(3, "")), "exit code 3");
        assert_eq!(
            detail("status = \"s\"\nup_pattern = \"On\"", exited(0, "On\n")),
            "output matches `On`"
        );
        assert_eq!(
            detail("status = \"s\"\nup_pattern = \"On\"", exited(0, "Off\n")),
            "output does not match `On`"
        );
        assert_eq!(
            detail("status = \"s\"\nup_pattern = \"On\"", exited(127, "")),
            "status command not found (exit code 127)"
        );
    }

    #[test]
    fn default_codes() {
        let v = vpn("status = \"s\"");
        assert_eq!(classify(&v, &exited(0, "")), VpnState::Up);
        assert_eq!(classify(&v, &exited(1, "")), VpnState::Down);
        assert_eq!(classify(&v, &exited(3, "")), VpnState::Unknown);
        let killed = Probe::Exited {
            code: None,
            output: String::new(),
        };
        assert_eq!(classify(&v, &killed), VpnState::Unknown);
    }

    #[test]
    fn custom_codes() {
        let v = vpn("status = \"s\"\nup_codes = [5]");
        assert_eq!(classify(&v, &exited(5, "")), VpnState::Up);
        assert_eq!(classify(&v, &exited(0, "")), VpnState::Unknown);
    }

    #[test]
    fn no_status_command_is_unknown() {
        let v = vpn("");
        assert_eq!(classify(&v, &exited(0, "")), VpnState::Unknown);
    }

    #[test]
    fn timed_out_and_failed_are_unknown() {
        let v = vpn("status = \"s\"\nup_pattern = \".*\"");
        assert_eq!(classify(&v, &Probe::TimedOut), VpnState::Unknown);
        assert_eq!(classify(&v, &Probe::Failed("x".into())), VpnState::Unknown);
    }

    struct FakeRunner;

    impl Runner for FakeRunner {
        fn probe(&self, cmd: &str, _timeout: Duration) -> Probe {
            match cmd {
                "up" => exited(0, ""),
                "down" => exited(1, ""),
                _ => Probe::TimedOut,
            }
        }

        fn run(&self, _cmd: &str) -> std::io::Result<std::process::ExitStatus> {
            unreachable!("probe_all must not run interactive commands")
        }
    }

    #[test]
    fn probe_all_keeps_order() {
        let config = Config::from_toml(
            r#"
            [[vpn]]
            name = "a"
            up = "u"
            down = "d"
            status = "down"
            [[vpn]]
            name = "b"
            up = "u"
            down = "d"
            [[vpn]]
            name = "c"
            up = "u"
            down = "d"
            status = "up"
            [[vpn]]
            name = "d"
            up = "u"
            down = "d"
            status = "hang"
            "#,
        )
        .unwrap();
        assert_eq!(
            probe_all(&config.vpns, &FakeRunner)
                .iter()
                .map(|o| o.state)
                .collect::<Vec<_>>(),
            [
                VpnState::Down,
                VpnState::Unknown,
                VpnState::Up,
                VpnState::Unknown
            ]
        );
    }
}
