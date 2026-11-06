# Changelog

All notable changes to this project are documented here.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and the project uses [Semantic Versioning](https://semver.org/).

## [0.2.0] - 2026-11-06

### Added
- `-v` / `--verbose` explains how each VPN state was determined
  (`exit code 1`, `output matches …`, `status command timed out after 10s`…).
- Prebuilt Linux binaries (x86_64 and aarch64) attached to each GitHub release.

### Changed
- During a switch, a VPN whose state is unknown is stopped on a best-effort
  basis: if its down command fails, `vpns` warns and carries on. An explicit
  `vpns down <name>` stays strict.
- A failed up/down command reports its exit code or signal.
- Ctrl-C during an up/down command lets that command end, then shows the
  resulting state and exits with an error instead of quitting silently.
- The picker honours `NO_COLOR`.

### Fixed
- An empty `VPNS_CONFIG` is treated as unset instead of breaking every command.
- A VPN with an empty name is rejected.
- Errors reading the configuration file name it once, not twice.
- Library: `switch::execute` returns an error instead of panicking on an
  out-of-range VPN index.

## [0.1.0] - 2026-11-06

Initial release: interactive picker, `status`, `up`, `down`, `off` and `init`
commands, TOML configuration with exit-code or regex state detection, status
probes run in parallel with a 10 s timeout.

[0.2.0]: https://github.com/AdrienGras/vpn-switcher/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/AdrienGras/vpn-switcher/releases/tag/v0.1.0
