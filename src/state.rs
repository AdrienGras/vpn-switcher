//! VPN state detection.

use crate::config::Vpn;
use crate::runner::{Probe, Runner, STATUS_TIMEOUT};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VpnState {
    Up,
    Down,
    Unknown,
}

/// Interprets the result of a VPN's status command.
pub fn classify(vpn: &Vpn, probe: &Probe) -> VpnState {
    if vpn.status.is_none() {
        return VpnState::Unknown;
    }
    let Probe::Exited { code, output } = probe else {
        return VpnState::Unknown;
    };
    if let Some(pattern) = &vpn.up_pattern {
        return if pattern.is_match(output) {
            VpnState::Up
        } else {
            VpnState::Down
        };
    }
    match code {
        Some(code) if vpn.up_codes.contains(code) => VpnState::Up,
        Some(code) if vpn.down_codes.contains(code) => VpnState::Down,
        _ => VpnState::Unknown,
    }
}

/// Probes every VPN in parallel; results follow the configuration order.
pub fn probe_all(vpns: &[Vpn], runner: &dyn Runner) -> Vec<VpnState> {
    std::thread::scope(|scope| {
        let handles: Vec<_> = vpns
            .iter()
            .map(|vpn| {
                scope.spawn(move || match &vpn.status {
                    Some(cmd) => classify(vpn, &runner.probe(cmd, STATUS_TIMEOUT)),
                    None => VpnState::Unknown,
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap_or(VpnState::Unknown))
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

        fn run(&self, _cmd: &str) -> std::io::Result<bool> {
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
            probe_all(&config.vpns, &FakeRunner),
            [
                VpnState::Down,
                VpnState::Unknown,
                VpnState::Up,
                VpnState::Unknown
            ]
        );
    }
}
