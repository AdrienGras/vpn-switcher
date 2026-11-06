# vpn-switcher

[![CI](https://github.com/AdrienGras/vpn-switcher/actions/workflows/ci.yml/badge.svg)](https://github.com/AdrienGras/vpn-switcher/actions/workflows/ci.yml)

`vpns` switches between VPNs that cannot run at the same time — say Proton VPN,
Tailscale and NetBird on the same laptop. Pick one with the arrow keys: the
active one goes down, the chosen one comes up.

```
◆ Switch to
  ○ proton     down
  ○ tailscale  down
› ● netbird    up
  ⏻ Disconnect all
[↑↓ move · enter switch · esc cancel]
```

Each VPN is driven by its own CLI, through shell commands you declare in a
small TOML file, so any VPN with a command-line client works.

## Install

Prebuilt Linux binaries (x86_64, aarch64) are attached to each
[release](https://github.com/AdrienGras/vpn-switcher/releases):

```sh
curl -sSL https://github.com/AdrienGras/vpn-switcher/releases/latest/download/vpns-v0.2.0-x86_64-unknown-linux-gnu.tar.gz \
  | tar xz --strip-components=1 -C ~/.local/bin vpns-v0.2.0-x86_64-unknown-linux-gnu/vpns
```

Or build from source (Rust 1.99 or newer):

```sh
cargo install --git https://github.com/AdrienGras/vpn-switcher
```

Linux is the primary target; any Unix should work.

## Usage

```sh
vpns init            # write an example config, then edit it
vpns                 # interactive picker (same as `vpns switch`)
vpns status          # state of every VPN
vpns up tailscale    # switch: stop the others, then start tailscale
vpns down tailscale  # stop one VPN
vpns off             # stop every VPN
vpns -v status       # also explain how each state was determined
```

When switching, every other VPN that is up — or whose state cannot be
determined — is stopped first, then the target is started, and the resulting
state is shown. If a command fails, `vpns` stops right there, reports its exit
code and exits with a non-zero code. The one exception: stopping a VPN whose
state is unknown is best-effort, since it was probably down already — if that
fails, `vpns` warns and carries on.

Up and down commands run attached to your terminal, so `sudo` can ask for a
password. Ctrl-C during one of them lets it end, then `vpns` shows the
resulting state. In the picker, Esc or Ctrl-C cancels without touching
anything. Colors follow `NO_COLOR`.

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

## License

MIT
