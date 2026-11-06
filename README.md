# vpn-switcher

Switch between VPNs that cannot run at the same time, with one command.

![vpns demo](https://raw.githubusercontent.com/AdrienGras/vpn-switcher/main/demo/demo.gif)

[![crates.io](https://img.shields.io/crates/v/vpn-switcher.svg)](https://crates.io/crates/vpn-switcher)
[![downloads](https://img.shields.io/crates/d/vpn-switcher.svg)](https://crates.io/crates/vpn-switcher)
[![CI](https://github.com/AdrienGras/vpn-switcher/actions/workflows/ci.yml/badge.svg)](https://github.com/AdrienGras/vpn-switcher/actions/workflows/ci.yml)
[![MSRV](https://img.shields.io/crates/msrv/vpn-switcher)](https://crates.io/crates/vpn-switcher)
[![license](https://img.shields.io/crates/l/vpn-switcher.svg)](https://github.com/AdrienGras/vpn-switcher/blob/main/LICENSE)

## Why

If you work for several clients, or split your time between companies, your
laptop ends up with a pile of VPNs: one OpenVPN or WireGuard profile per client,
an AnyConnect or Fortinet gateway, plus Tailscale, NetBird or Proton for
yourself. Most of them cannot be up together: routes and DNS collide, and
whichever connected last wins, or nothing works at all.

`vpns` makes the switch one step. Pick a VPN with the arrow keys: every other
one that is up goes down, then the chosen one comes up. Each VPN is driven by
its own CLI through shell commands you declare in a small TOML file, so
anything with a command-line client works. `vpns init` writes that file for you
from the VPNs it finds on your machine.

## Install

```sh
cargo binstall vpn-switcher     # prebuilt binary, no compilation
cargo install vpn-switcher      # from source, needs Rust 1.87 or newer
```

Or download an archive from the [releases page](https://github.com/AdrienGras/vpn-switcher/releases). Archives are
built for `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl`,
`x86_64-apple-darwin` and `aarch64-apple-darwin`. Linux binaries are static and
run on any distribution.

```sh
target=x86_64-unknown-linux-musl   # or aarch64-unknown-linux-musl, x86_64-apple-darwin, aarch64-apple-darwin
mkdir -p ~/.local/bin
curl -sSL "https://github.com/AdrienGras/vpn-switcher/releases/latest/download/vpns-$target.tar.gz" \
  | tar xz --strip-components=1 -C ~/.local/bin "vpns-$target/vpns"
```

Each archive also holds shell completions, the man page (`vpns.1`), the
README, the license and the changelog.

Check where an archive comes from (build provenance attestation, needs the
[GitHub CLI](https://cli.github.com/)):

```sh
gh attestation verify vpns-<target>.tar.gz --repo AdrienGras/vpn-switcher
```

### Shell completions

```sh
# bash
vpns completions bash > ~/.local/share/bash-completion/completions/vpns

# zsh (any directory of your $fpath; this one must exist and be in it)
vpns completions zsh > ~/.zfunc/_vpns

# fish
vpns completions fish > ~/.config/fish/completions/vpns.fish
```

Open a new shell afterwards. `elvish` and `powershell` scripts are available
too.

## Quick start

```sh
vpns init            # detect the installed VPNs and write the config
vpns config check    # validate it and test every status command
vpns                 # pick a VPN
```

`vpns init` lists what it found (Proton VPN, Tailscale, NetBird, NetworkManager
connections, WireGuard and OpenVPN profiles, Mullvad, WARP, macOS network
services, and templates for OpenConnect and openfortivpn) and lets you tick the
ones to keep. It only looks: it never starts or stops a VPN. If a config file
already exists, `init` appends the VPNs it lacks and leaves the rest of your
file alone. Without a terminal, or with `--example`, it writes a commented
example instead; `--force` overwrites an existing file.

Presets that were not tried on a real VPN are marked in the generated file.
`vpns config check` tells you whether each status command is readable.

## Usage

```sh
vpns init            # detect installed VPNs and write the config
vpns init --example  # write a commented example instead
vpns                 # interactive picker (same as `vpns switch`)
vpns status          # state of every VPN
vpns up tailscale    # switch: stop the others, then start tailscale
vpns down tailscale  # stop one VPN
vpns off             # stop every VPN
vpns -v status       # also explain how each state was determined
vpns config path     # print the config file path in use
vpns config edit     # open it in $VISUAL / $EDITOR, then check it
vpns config check    # validate it and test every status command
vpns completions zsh # print a completion script (bash, zsh, fish, elvish, powershell)
```

When switching, every other VPN that is up, or whose state cannot be
determined, is stopped first, then the target is started, and the resulting
state is shown. If a command fails, `vpns` stops right there, reports its exit
code and exits with a non-zero code. The one exception: stopping a VPN whose
state is unknown is best-effort, since it was probably down already. If that
fails, `vpns` warns, carries on, and still exits with a non-zero code since that
VPN may be up.

Up and down commands run attached to your terminal, so `sudo` can ask for a
password. Ctrl-C (or SIGTERM) during a switch stops the running command, skips
the remaining steps, shows the resulting state and exits with code 130; a second
one quits at once. In the picker, Esc or Ctrl-C cancels without touching
anything. Colors follow `NO_COLOR` and `FORCE_COLOR`.

`vpns config check` prints one line per VPN (name, state, reason). It exits
with 0 if the file is valid and every VPN that has a `status` command is
readable (up or down), 1 otherwise. A VPN without a `status` command is shown
as unknown and does not fail the check.

`vpns config edit` honors `$VISUAL`, then `$EDITOR`, then `vi`; the variable
may contain arguments (`code --wait`).

## Presets

These are the entries `vpns init` writes. Copy one into your config by hand if
you prefer. "Verified" means the commands and the status output were observed
on a real machine; the others come from the tools' documentation, so run
`vpns config check` after adding them. Replace `<profile>` with the profile
name, quoted for `sh` (`'ACME (Paris)'`). Presets that start a root service use
`sudo`; `status` commands never do, because they run without a terminal.

### Proton VPN (Linux, verified)

```toml
[[vpn]]
name = "proton"
up = "protonvpn connect"
down = "protonvpn disconnect"
status = "protonvpn status"
up_pattern = "Status: Connected"
```

### Tailscale (Linux verified, macOS not)

```toml
[[vpn]]
name = "tailscale"
up = "tailscale up"
down = "tailscale down"
status = "tailscale status"   # exit 0 = up, 1 = down
```

On macOS, when `tailscale` is not in `PATH`, `init` uses the app's binary by
absolute path: `/Applications/Tailscale.app/Contents/MacOS/Tailscale`.

### NetBird (Linux verified, macOS not)

```toml
[[vpn]]
name = "netbird"
up = "netbird up"
down = "netbird down"
status = "netbird status"
up_pattern = "Management: Connected"
```

### NetworkManager: OpenVPN, WireGuard and other connections (Linux, partly verified)

One entry per connection of type `vpn` or `wireguard` listed by
`nmcli -t -f NAME,TYPE connection show`. The inactive state was observed; the
active one was not.

```toml
[[vpn]]
name = "acme"
up = "nmcli connection up id '<profile>'"
down = "nmcli connection down id '<profile>'"
status = "nmcli -g GENERAL.STATE connection show id '<profile>'"
up_pattern = "(?m)^activated$"
```

### wg-quick (Linux, not verified)

One entry per `/etc/wireguard/*.conf` when that directory is readable (it is
usually root-only, so `init` often writes a commented template instead).
`ip link show` exits 0 when the interface exists, 1 otherwise.

```toml
[[vpn]]
name = "globex-wg"
up = "sudo wg-quick up '<profile>'"
down = "sudo wg-quick down '<profile>'"
status = "ip link show '<profile>'"
```

### OpenVPN with systemd (Linux, not verified)

One entry per `/etc/openvpn/client/*.conf` when readable, otherwise a template.
`systemctl is-active` exits 3 for an inactive unit.

```toml
[[vpn]]
name = "acme-ovpn"
up = "sudo systemctl start 'openvpn-client@<profile>'"
down = "sudo systemctl stop 'openvpn-client@<profile>'"
status = "systemctl is-active --quiet 'openvpn-client@<profile>'"
down_codes = [3]
```

### OpenConnect: AnyConnect, GlobalProtect (Linux, macOS, not verified)

Always a template, since the server address is yours.

```toml
[[vpn]]
name = "openconnect"
up = "sudo openconnect --background --protocol=anyconnect vpn.example.com"   # --protocol=gp for GlobalProtect
down = "sudo pkill -x openconnect"
status = "pgrep -x openconnect"
```

### openfortivpn: Fortinet (Linux, macOS, not verified)

Always a template. Replace the server, or use `-c /etc/openfortivpn/config`.

```toml
[[vpn]]
name = "openfortivpn"
up = "sudo sh -c 'openfortivpn vpn.example.com:443 >/dev/null 2>&1 &'"
down = "sudo pkill -x openfortivpn"
status = "pgrep -x openfortivpn"
```

### Mullvad (Linux, macOS, not verified)

```toml
[[vpn]]
name = "mullvad"
up = "mullvad connect"
down = "mullvad disconnect"
status = "mullvad status"
up_pattern = "^Connected"
```

### Cloudflare WARP (Linux, macOS, not verified)

The `up_pattern` is unconfirmed.

```toml
[[vpn]]
name = "warp"
up = "warp-cli connect"
down = "warp-cli disconnect"
status = "warp-cli status"
up_pattern = "Status update: Connected"
```

### macOS network services (macOS, not verified)

One entry per service listed by `scutil --nc list` (the VPNs of System
Settings).

```toml
[[vpn]]
name = "acme-ikev2"
up = "scutil --nc start '<service>'"
down = "scutil --nc stop '<service>'"
status = "scutil --nc status '<service>'"
up_pattern = "^Connected"
```

## Configuration

The file lives at `$XDG_CONFIG_HOME/vpn-switcher/config.toml`
(`~/.config/vpn-switcher/config.toml` by default). Override it with
`--config <path>` or the `VPNS_CONFIG` environment variable (an empty value
counts as unset).

```toml
[[vpn]]
name = "proton"
up = "protonvpn connect"
down = "protonvpn disconnect"
status = "protonvpn status"
up_pattern = "Status: Connected"

[[vpn]]
name = "tailscale"
up = "tailscale up"           # prefix with sudo if you are not the tailscale operator
down = "tailscale down"
status = "tailscale status"   # exit 0 = up, 1 = down (defaults)

[[vpn]]
name = "netbird"
up = "netbird up"
down = "netbird down"
status = "netbird status"
up_pattern = "Management: Connected"
```

| Key          | Required | Meaning                                                        |
|--------------|----------|----------------------------------------------------------------|
| `name`       | yes      | Identifier used on the command line; unique, not empty.        |
| `up`         | yes      | Shell command starting the VPN.                                |
| `down`       | yes      | Shell command stopping the VPN.                                |
| `status`     | no       | Shell command reporting the state. Without it, the state is `unknown`. |
| `up_pattern` | no       | Regex: the VPN is up when the status output (stdout + stderr) matches. |
| `up_codes`   | no       | Status exit codes meaning "up". Default `[0]`.                 |
| `down_codes` | no       | Status exit codes meaning "down". Default `[1]`.               |

How the state is read from the `status` command:

1. if `up_pattern` is set, a match means **up**, anything else **down** — unless
   the command was not found (126/127) or was killed, which means **unknown**;
2. otherwise an exit code in `up_codes` means **up**, in `down_codes` **down**,
   anything else **unknown**.

Some clients (Proton VPN, NetBird) exit with `0` whether connected or not —
use `up_pattern` for those. Status commands run in parallel, without a
terminal, and are killed after 10 seconds (state `unknown`). All commands run
through `sh -c`.

## FAQ

**Do I need `sudo`? Where do I type the password?**
Up and down commands run attached to your terminal, so `sudo` prompts as
usual. Status commands run without a terminal and must never need a password.
To avoid prompts, add a `sudoers` rule for the exact commands, or use a client
that does not need root.

**`tailscale up` says I need `sudo` or to be the operator.**
Either prefix `up` and `down` with `sudo`, or make your user the operator once:
`sudo tailscale set --operator=$USER`.

**My VPN has no status command.**
Leave `status` out. Its state is then `unknown`, so `vpns` always runs its down
command during a switch (best-effort) and never knows whether it is up. If the
client has no status at all, a process or interface check works:
`pgrep -x openvpn`, `ip link show <iface>`.

**A client exits 0 whether it is connected or not.**
Match its output with `up_pattern` (Proton VPN and NetBird do this). Run the
status command by hand and pick a line that appears only when connected.

**I have several profiles for the same tool.**
Add one `[[vpn]]` per profile, each with a distinct `name`. `vpns init` does
this for NetworkManager, WireGuard and OpenVPN profiles.

**What are the exit codes?**
`0` on success, and when you cancel the picker or `init` with Esc or Ctrl-C.
`1` on any failure: invalid config, unknown VPN, a failed up or down command, a
failed `config check`. `130` when a switch was interrupted by Ctrl-C, SIGTERM
or SIGHUP.

**Where does the state come from?**
Run `vpns -v status`: it prints the reason for each state (exit code, pattern
match, timeout).

**Windows?**
Not supported.

## Platforms

Linux and macOS. Windows is not supported.

## License

[MIT](https://github.com/AdrienGras/vpn-switcher/blob/main/LICENSE)
