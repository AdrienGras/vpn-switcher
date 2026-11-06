# Changelog

All notable changes to this project are documented here.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and the project uses [Semantic Versioning](https://semver.org/).

## [0.3.0] - 2026-11-06

### Added
- `vpns init` detects the installed VPNs (Proton VPN, Tailscale, NetBird,
  NetworkManager connections, WireGuard and OpenVPN profiles, OpenConnect,
  openfortivpn, Mullvad, Cloudflare WARP, macOS network services) and writes
  presets for the ones you pick. Detection is read-only. When a config file
  already exists, `init` appends only the VPNs it lacks.
- `vpns init --example` writes the commented example; `--force` overwrites.
- `vpns config path`, `vpns config edit` and `vpns config check`. `check`
  validates the file and tests every status command.
- `vpns completions <shell>` for bash, zsh, fish, elvish and PowerShell, and a
  man page, both shipped in the release archives.
- macOS binaries (x86_64 and aarch64) next to the Linux ones.
- `cargo binstall vpn-switcher` and crates.io publishing.
- Build provenance attestations for the release archives
  (`gh attestation verify`).
- README with presets for each detected VPN, FAQ and demo GIF; `SECURITY.md`
  and `RELEASING.md`.

### Changed
- Minimum supported Rust version is 1.87.
- The library API is hidden from the documentation: `vpn-switcher` is a
  command-line tool, not a library.

## [0.2.0] - 2026-11-06

### Added
- `-v` / `--verbose` explains how each VPN state was determined
  (`exit code 1`, `output matches …`, `status command timed out after 10s`…).
- Static Linux binaries (x86_64 and aarch64, musl) attached to each GitHub release.

### Changed
- During a switch, a VPN whose state is unknown is stopped on a best-effort
  basis: if its down command fails, `vpns` warns, carries on, and exits with a
  non-zero code. An explicit `vpns down <name>` stays strict.
- A failed up/down command reports its exit code or signal.
- Ctrl-C, SIGTERM or SIGHUP during a switch stops the running command, skips
  the remaining steps, shows the resulting state and exits with code 130; a
  second signal quits at once.
- The picker follows the same color rules as the rest of the output
  (`NO_COLOR`, `FORCE_COLOR`).

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

[0.3.0]: https://github.com/AdrienGras/vpn-switcher/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/AdrienGras/vpn-switcher/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/AdrienGras/vpn-switcher/releases/tag/v0.1.0
