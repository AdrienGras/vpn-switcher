# vpn-switcher 0.3.0 Public Release Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `vpns` a public product: auto-detecting `init`, `config path|edit|check`, completions and man page, crates.io + binstall distribution built by CI for Linux and macOS, and an attractive README.

**Architecture:** Rust CLI (lib + thin bin). New module `detect` (pure presets + read-only detectors behind a `Sources` struct for testability); new subcommands wired in `cli.rs`/`main.rs`; `ui.rs` gains a multi-select and a check report. Release pipeline stays hand-written in GitHub Actions, with packaging moved into a shell script testable locally.

**Tech Stack:** Rust 2024, clap 4 (+ clap_complete, clap_mangen), inquire 0.9 (`MultiSelect`), toml, GitHub Actions, cargo-deny, cargo-binstall, vhs.

**Spec:** `docs/superpowers/specs/2026-11-06-public-release-design.md` (French). Read it before any task.

## Global Constraints

- Branch `release-0.3.0`. Every commit message ends with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- `rust-version = "1.87"`; edition 2024; `cargo clippy --all-targets -- -D warnings` and `cargo fmt --all --check` clean after every task.
- All existing tests (59) stay green. Run tests with `rtk proxy cargo test` to see real output (an `rtk` hook summarizes plain `cargo test`).
- README, CLI output, code comments: English. Archive name `vpns-<target>.tar.gz` with folder `vpns-<target>/` — unchanged.
- Never run up/down of the real VPNs on this machine (protonvpn, tailscale, netbird, `nmcli connection up/down`). Tests use fakes only (`touch`/`rm`/`test -f` in a temp dir, fake runners, fake binaries in a temp `PATH`).
- Detection is read-only: it never runs up/down, never `sudo`, never anything that changes network state.
- `status` commands in presets never use `sudo` (they run without a terminal).
- Profile names injected into shell commands are single-quoted for `sh`; values written to TOML go through the `toml` crate's string encoding.
- `vpns init` never overwrites an existing file without `--force`.
- Windows is out of scope.

## Review Focus

1. **A profile name with spaces, quotes, parentheses or non-ASCII** (`ACME (Paris) — l'agence`): the written TOML must parse and the shell command must address exactly that profile. → Task 5 test `preset_with_hostile_name_parses_and_quotes`.
2. **`init` appending to a hand-written file** without a trailing newline and with comments: original bytes are preserved and the result parses. → Task 7 test `append_preserves_existing_bytes`.
3. **`init` on an existing but invalid config**: refuses to append (would hide the error), writes nothing, tells the user to fix it or use `--force`. → Task 7 test `append_refuses_invalid_existing`.
4. **`$EDITOR` with arguments and a config path containing a space** (`code --wait`, `~/My Config/config.toml`). → Task 3 test `config_edit_passes_path_with_space_to_editor_with_args`.
5. **A detector whose listing command fails or hangs** (nmcli not running, D-Bus down): that detector contributes nothing, the others still work, no error. → Task 6 test `failed_listing_drops_only_that_detector`.

---

### Task 1: CI matrix (Linux + macOS), MSRV 1.87, cargo-deny, dependabot, draft PR

**Files:**
- Modify: `.github/workflows/ci.yml`, `Cargo.toml` (`rust-version`)
- Create: `deny.toml`, `.github/dependabot.yml`

**Interfaces:** Produces the CI jobs every later task relies on for macOS feedback.

- [ ] **Step 1: Measure the MSRV locally**

Run: `rustup toolchain install 1.87 --profile minimal && cargo +1.87 test --locked` (after setting `rust-version = "1.87"`).
Expected: all tests pass. If not, find the real floor (bisect 1.87→1.99 by minor) and use it everywhere this plan says 1.87; tell the user.

- [ ] **Step 2: Rewrite `ci.yml`**

Jobs (all `actions/checkout@v5`, `Swatinem/rust-cache@v2`, `CARGO_TERM_COLOR: always`):
- `check` (ubuntu-latest, stable + rustfmt/clippy): `cargo fmt --all --check`, `cargo clippy --all-targets --locked -- -D warnings`.
- `test` matrix `os: [ubuntu-latest, macos-latest]`, stable: `cargo test --locked`.
- `msrv` (ubuntu-latest, `dtolnay/rust-toolchain@1.87`): `cargo test --locked`.
- `deny` (ubuntu-latest): `EmbarkStudios/cargo-deny-action@v2`.

- [ ] **Step 3: Write `deny.toml`**

Sections `[advisories]`, `[licenses]` (allow exactly the licenses `cargo deny list` reports for the current tree, e.g. MIT, Apache-2.0, Unicode-3.0, Zlib, BSD-3-Clause; `confidence-threshold = 0.9`), `[bans] multiple-versions = "warn"`, `[sources] unknown-registry = "deny"`, `unknown-git = "deny"`.
Run: `cargo deny check` (install with `cargo binstall cargo-deny` if missing). Expected: `advisories ok, bans ok, licenses ok, sources ok`.

- [ ] **Step 4: Write `.github/dependabot.yml`**

Two ecosystems, `cargo` and `github-actions`, `directory: "/"`, `schedule.interval: weekly`, one `groups` entry each matching `"*"`.

- [ ] **Step 5: Lint workflows**

Run: `actionlint` at repo root. Expected: no output.

- [ ] **Step 6: Commit, push, open the draft PR**

```bash
git add .github deny.toml Cargo.toml
git commit -m "ci: test on Linux and macOS, MSRV 1.87, cargo-deny, dependabot"
git push -u origin release-0.3.0
gh pr create --draft --title "0.3.0: public release" --body "Implements docs/superpowers/specs/2026-11-06-public-release-design.md"
```
Link the PR in T3 (`link_pull_request`).

- [ ] **Step 7: Get macOS green**

Check with `gh run list --branch release-0.3.0` / `gh run view <id>` (no `--watch`, no polling loop; if the run is still going, start Task 2 and come back).
Expected: all jobs pass. Any macOS failure is diagnosed with superpowers:systematic-debugging and fixed in code or tests (not skipped with `cfg`), in its own commit.

---

### Task 2: Hidden lib, crate packaging, binstall metadata

**Files:**
- Modify: `src/lib.rs`, `Cargo.toml`

- [ ] **Step 1: Hide the lib**

`src/lib.rs`: crate doc says the library is an implementation detail of the `vpns` binary with no semver guarantee, and points to the README; every `pub mod` gets `#[doc(hidden)]`.

- [ ] **Step 2: Restrict the package and add binstall metadata**

`Cargo.toml`: `include = ["src/**/*", "tests/**/*", "Cargo.toml", "Cargo.lock", "README.md", "LICENSE", "CHANGELOG.md"]`, and the exact `[package.metadata.binstall]` block from the spec (`pkg-url = "{ repo }/releases/download/v{ version }/vpns-{ target }.tar.gz"`, `bin-dir = "vpns-{ target }/{ bin }{ binary-ext }"`, `pkg-fmt = "tgz"`).

- [ ] **Step 3: Verify the package contents**

Run: `cargo package --list --allow-dirty`
Expected: only paths under `src/`, `tests/`, plus `Cargo.toml`, `Cargo.toml.orig`, `Cargo.lock`, `README.md`, `LICENSE`, `CHANGELOG.md`, `.cargo_vcs_info.json`.

- [ ] **Step 4: Verify binstall against the published v0.2.0 archives** (version is still 0.2.0 here)

Run: `d=$(mktemp -d) && cargo binstall --manifest-path . --root "$d" --no-confirm --disable-strategies compile vpn-switcher && "$d/bin/vpns" --version`
Expected: `vpns 0.2.0`. Nothing installed outside `$d`.

- [ ] **Step 5: Commit** — `build: hide the library API, restrict the package, add binstall metadata`

---

### Task 3: `vpns config path|edit|check`

**Files:**
- Modify: `src/cli.rs`, `src/main.rs`, `src/state.rs`, `src/ui.rs`
- Test: `tests/cli.rs`, unit tests in `src/state.rs`

**Interfaces:**
- Produces: `cli::Command::Config { action: ConfigAction }`, `pub enum ConfigAction { Path, Edit, Check }` (doc comments: "Print the configuration file path", "Open the configuration in $VISUAL or $EDITOR, then check it", "Validate the configuration and test every status command").
- Produces: `state::unreadable(vpns: &[Vpn], observations: &[Observation]) -> Vec<usize>` — indices of VPNs that **have** a `status` command and came out `Unknown`.
- Produces: `ui::print_check(path: &Path, vpns: &[Vpn], observations: &[Observation])` — header `✓ <path>: <n> VPNs, valid` (`1 VPN` singular), then the verbose status rows (reuse `print_status(.., true)`).
- Consumes: existing `load`, `Context::probe`/`probe_with_spinner`, `config::resolve_path`.

- [ ] **Step 1: Unit test `unreadable`** in `src/state.rs`

```rust
#[test]
fn unreadable_ignores_vpns_without_status() {
    // vpns: a (status, Unknown), b (no status, Unknown), c (status, Down)
    assert_eq!(unreadable(&vpns, &obs), vec![0]);
}
```

- [ ] **Step 2: Integration tests** in `tests/cli.rs` (use the existing `setup`/`vpns` helpers)

- `config_path_prints_resolved_path`: `vpns -c <dir>/config.toml config path` → stdout is exactly that path + `\n`; with a missing file, still exit 0 and stderr contains `does not exist yet` and `vpns init`.
- `config_check_passes_when_every_status_is_readable`: alpha `status = "true"`, beta `status = "false"`, gamma without status → success; stdout contains `3 VPNs, valid`, `gamma` row contains `no status command`.
- `config_check_fails_on_unreadable_status`: alpha `status = "exit 7"` → failure (exit 1); stdout contains `exit code 7`; stderr contains `1 VPN cannot be read: its status command is probably wrong`. Plural for 2: `2 VPNs cannot be read: their status commands are probably wrong`.
- `config_check_fails_on_missing_command_with_pattern`: `status = "no-such-cmd-xyz"`, `up_pattern = "x"` → failure; stdout contains `not found`.
- `config_check_rejects_invalid_config`: duplicate names → failure, stderr contains `defined more than once`.
- `config_edit_runs_editor_then_check`: `VISUAL` unset, `EDITOR = "true"` → success and stdout contains `valid`.
- `config_edit_prefers_visual`: `VISUAL="sh <dir>/ed.sh"` where `ed.sh` writes `$1` into `<dir>/seen`; `EDITOR="false"` → success, `seen` contains the config path.
- `config_edit_passes_path_with_space_to_editor_with_args` (Review Focus 4): config at `<dir>/My Config/config.toml`, `EDITOR="sh <dir>/ed.sh --wait"` where `ed.sh` writes `$1|$2` → `seen` is `--wait|<dir>/My Config/config.toml`.
- `config_edit_without_file_suggests_init`: failure, stderr contains `vpns init`.

- [ ] **Step 3: Run them, expect FAIL** (`rtk proxy cargo test --test cli config_`): unknown subcommand `config`.

- [ ] **Step 4: Implement**

- `config path`: print the resolved path to stdout; if it does not exist, `eprintln!("note: {} does not exist yet; run `vpns init` to create it", …)`.
- `config edit`: editor = first non-empty of `$VISUAL`, `$EDITOR`, else `vi`; run with `std::process::Command::new("sh").args(["-c", &format!("{editor} \"$1\""), "vpns-edit"]).arg(&path)` attached to the terminal (status ignored except spawn errors); missing file → same error as `load`. Then run `check`.
- `config check`: `load` (errors propagate as today), probe with the spinner, `ui::print_check`, then if `unreadable` is non-empty return `Err(anyhow!(<message above>))` (exit 1 via the existing error path).

- [ ] **Step 5: Run all tests, expect PASS** — `rtk proxy cargo test`

- [ ] **Step 6: Commit** — `feat: vpns config path, edit and check`

---

### Task 4: Shell completions and man page

**Files:**
- Modify: `Cargo.toml` (deps `clap_complete = "4.6"`, `clap_mangen = "0.3"`), `src/cli.rs`, `src/main.rs`
- Test: `tests/cli.rs`

**Interfaces:**
- Produces: `Command::Completions { shell: clap_complete::Shell }` ("Print a shell completion script"), and `#[command(hide = true)] Command::Man` (prints the roff man page to stdout). Task 8's packaging script calls `vpns completions bash|zsh|fish` and `vpns man`.

- [ ] **Step 1: Tests**: `completions_bash_mentions_vpns` (success, stdout contains `_vpns`), `completions_rejects_unknown_shell` (`completions tcsh` fails), `man_page_is_roff` (success, stdout contains `.TH vpns`), `man_is_hidden_from_help` (no line of `--help` stdout matches `^\s+man\s`).
- [ ] **Step 2: Run, expect FAIL.**
- [ ] **Step 3: Implement** with `clap_complete::generate(shell, &mut Cli::command(), "vpns", &mut stdout)` and `clap_mangen::Man::new(Cli::command()).render(&mut stdout)`. Neither needs a config file: dispatch them before `config::resolve_path` so they work without `HOME`/XDG.
- [ ] **Step 4: Run all tests, PASS; `cargo +1.87 check --locked`** still passes (both crates declare 1.85).
- [ ] **Step 5: Commit** — `feat: shell completions and man page`

---

### Task 5: `detect` module — presets, naming, quoting, rendering (pure)

**Files:**
- Create: `src/detect.rs` (add `#[doc(hidden)] pub mod detect;` to `src/lib.rs`)

**Interfaces (produced, used by Tasks 6–7):**

```rust
pub struct Preset {
    pub name: String,          // vpns-side name, already sanitized
    pub source: String,        // shown in the list and the TOML comment, e.g. "NetworkManager (openvpn)"
    pub up: String,
    pub down: String,
    pub status: Option<String>,
    pub up_pattern: Option<String>,
    pub down_codes: Option<Vec<i32>>, // None = default
    pub verified: bool,
    pub template: bool,        // true = written fully commented out (OpenConnect, openfortivpn, unreadable dirs)
    pub note: Option<String>,  // extra comment line, e.g. "replace vpn.example.com with your server"
}
pub fn shell_quote(s: &str) -> String;           // 'it'\''s' style, always quoted
pub fn vpn_name(profile: &str) -> String;
pub fn dedupe(presets: Vec<Preset>, taken: &[&str]) -> Vec<Preset>;
pub fn render(preset: &Preset) -> String;        // one TOML block, ends with "\n"
```

`vpn_name` rule: lowercase; every run of characters outside `[a-z0-9._-]` becomes one `-`; trim `-` at both ends; empty → `vpn`. `dedupe`: in order, a name already in `taken` or earlier in the list gets `-2`, `-3`… (first free).
`render`: comment lines first — `# detected: <source>`, then `# preset not verified on a real VPN — run `vpns config check`` when `!verified`, then `# <note>`; then `[[vpn]]` and keys `name`, `up`, `down`, `status`, `up_pattern`, `down_codes`, values encoded with `toml::Value::String(..).to_string()` / arrays; when `template`, every line of the `[[vpn]]` table is prefixed with `# `.

- [ ] **Step 1: Unit tests**

- `shell_quote_escapes_single_quotes`: `shell_quote("l'agence") == "'l'\\''agence'"`; `shell_quote("") == "''"`.
- `vpn_name_examples`: `"ACME (Paris) — l'agence"` → `"acme-paris-l-agence"`; `"  "` → `"vpn"`; `"globex.wg"` → `"globex.wg"`.
- `dedupe_suffixes_collisions`: presets `tailscale, tailscale, acme` with `taken = ["acme"]` → `tailscale, tailscale-2, acme-2`.
- `preset_with_hostile_name_parses_and_quotes` (Review Focus 1): build an nmcli-style preset for profile `ACME (Paris) — l'agence` (up = `format!("nmcli connection up id {}", shell_quote(profile))`), `Config::from_toml(&render(&p))` succeeds, the parsed `up` equals the original string, and `sh -c "printf %s <quoted>"` run through `std::process::Command` prints the exact profile name.
- `unverified_preset_is_marked`: render contains `preset not verified`; a verified one does not.
- `template_is_fully_commented`: for `template: true`, `Config::from_toml(&render(&p))` fails with `Empty` (nothing active) and every non-empty line starts with `#`.
- `render_writes_down_codes`: `down_codes: Some(vec![3])` → parsed `down_codes == [3]`.

- [ ] **Step 2: Run, expect FAIL** (module missing).
- [ ] **Step 3: Implement** the four functions.
- [ ] **Step 4: Run, expect PASS.**
- [ ] **Step 5: Commit** — `feat(detect): presets, naming, quoting and TOML rendering`

---

### Task 6: Detectors

**Files:**
- Modify: `src/detect.rs`

**Interfaces:**
- Consumes: `Preset`, `shell_quote`, `vpn_name` (Task 5); `runner::{Runner, Probe}`.
- Produces:

```rust
pub struct Sources<'a> {
    pub os: &'a str,                 // std::env::consts::OS in production: "linux" | "macos"
    pub path: std::ffi::OsString,    // $PATH
    pub wireguard_dir: PathBuf,      // /etc/wireguard
    pub openvpn_dir: PathBuf,        // /etc/openvpn/client
    pub tailscale_app: PathBuf,      // /Applications/Tailscale.app/Contents/MacOS/Tailscale
    pub runner: &'a dyn Runner,
}
impl Sources<'_> { pub fn system(runner: &dyn Runner) -> Sources<'_>; }
pub fn detect(src: &Sources) -> Vec<Preset>;     // not yet deduped
```

Binary presence = an executable file named `<bin>` in one of `path`'s entries. Listing commands go through `src.runner.probe(cmd, Duration::from_secs(5))`; anything but `Probe::Exited { code: Some(0), .. }` drops that detector's profiles. Directory listing: `*.conf` files, sorted, profile = file stem; unreadable, missing or empty dir → one `template` preset (name = tool name, note says where profiles live).

Detector table — exact presets (Linux unless noted; `Q(n)` = `shell_quote(n)`):

| Detector | OS | Preset | verified |
|---|---|---|---|
| protonvpn | linux | name `proton`: `protonvpn connect` / `protonvpn disconnect` / `protonvpn status`, `up_pattern = "Status: Connected"` | true |
| tailscale | linux, macos (also `tailscale_app` on macos, used by absolute path quoted) | `tailscale up` / `tailscale down` / `tailscale status` | linux true, macos false |
| netbird | linux, macos | `netbird up` / `netbird down` / `netbird status`, `up_pattern = "Management: Connected"` | linux true, macos false |
| nmcli | linux | list `nmcli -t -f NAME,TYPE connection show`, keep TYPE `vpn`/`wireguard` (split on the **last** `:`; nmcli escapes `:` in names as `\:` — unescape `\:` and `\\`); per profile: `nmcli connection up id Q(n)` / `nmcli connection down id Q(n)` / `nmcli -g GENERAL.STATE connection show id Q(n)`, `up_pattern = "^activated$"`; source `NetworkManager (vpn)` or `NetworkManager (wireguard)`, from TYPE | false |
| wg-quick | linux | per `<n>.conf`: `sudo wg-quick up Q(n)` / `sudo wg-quick down Q(n)` / `ip link show Q(n)` | false |
| openvpn | linux | per `<n>.conf`: `sudo systemctl start Q(openvpn-client@n)` / `sudo systemctl stop …` / `systemctl is-active --quiet Q(openvpn-client@n)`, `down_codes = [3]` | false |
| openconnect | linux, macos | template `openconnect`: `sudo openconnect --background --protocol=anyconnect vpn.example.com` / `sudo pkill -x openconnect` / `pgrep -x openconnect`; note `replace vpn.example.com; use --protocol=gp for GlobalProtect` | false |
| openfortivpn | linux, macos | template `openfortivpn`: `sudo sh -c 'openfortivpn vpn.example.com:443 >/dev/null 2>&1 &'` / `sudo pkill -x openfortivpn` / `pgrep -x openfortivpn`; note `replace vpn.example.com:443, or use -c /etc/openfortivpn/config` | false |
| mullvad | linux, macos | `mullvad connect` / `mullvad disconnect` / `mullvad status`, `up_pattern = "^Connected"` | false |
| warp-cli | linux, macos | name `warp`: `warp-cli connect` / `warp-cli disconnect` / `warp-cli status`, `up_pattern = "Status update: Connected"` (check Cloudflare's docs; adjust and note the source in a code comment) | false |
| scutil | macos | list `scutil --nc list`, parse the quoted service name on each line starting with `*`; per service: `scutil --nc start Q(n)` / `scutil --nc stop Q(n)` / `scutil --nc status Q(n)`, `up_pattern = "^Connected"` | false |

Order of the returned list = order of this table. The protonvpn/tailscale/netbird presets must equal what `config::EXAMPLE` contains (a test enforces it).

- [ ] **Step 1: Tests** (temp dir as `path` holding empty `chmod +x` files; a `FakeRunner` mapping command → `Probe`)

- `nothing_installed_detects_nothing`.
- `linux_vendor_clis_match_example`: fake `protonvpn tailscale netbird` → 3 presets, verified, and their rendered tables parse to the same `up/down/status/up_pattern` as the matching VPNs in `Config::from_toml(EXAMPLE)`.
- `nmcli_lists_vpn_and_wireguard_profiles`: fake runner output `acme:vpn\nWi-Fi\\:home:802-11-wireless\nglobex wg:wireguard\n` → 2 presets `acme`, `globex-wg`, sources `NetworkManager (vpn)`/`(wireguard)`.
- `nmcli_unescapes_colons`: `a\:b:vpn` → profile `a:b` quoted in commands.
- `failed_listing_drops_only_that_detector` (Review Focus 5): nmcli probe returns `Probe::TimedOut` while `tailscale` exists → exactly the tailscale preset.
- `wireguard_dir_lists_conf_files` (dir with `b.conf`, `a.conf`, `notes.txt`) → `a`, `b`; `unreadable_wireguard_dir_gives_template` (missing dir) → one `template` preset.
- `openvpn_uses_systemd_unit_and_code_3`.
- `macos_detectors_only_on_macos`: `scutil` and `nmcli` both present; `os = "macos"` → scutil profiles only, `os = "linux"` → nmcli only. `scutil --nc list` sample:
  `* (Disconnected)   1A2B-… PPP --> L2TP  "Office VPN"  [PPP/L2TP]` → profile `Office VPN`.
- [ ] **Step 2: Run, expect FAIL.**
- [ ] **Step 3: Implement** `Sources::system`, `detect` and one private fn per detector.
- [ ] **Step 4: Run, expect PASS.** Then a real, read-only smoke run on this machine: a `#[test] #[ignore]` `smoke_detect_on_this_machine` printing `detect(&Sources::system(&ShellRunner))` names; run it with `rtk proxy cargo test smoke_detect -- --ignored --nocapture`. Expected here: `proton`, `tailscale`, `netbird`, `adrien.gras.nomonkeybusiness` (nmcli), a `wg-quick` template (dir unreadable), an `openvpn` template (dir empty). Do not run anything else.
- [ ] **Step 5: Commit** — `feat(detect): detectors for vendor CLIs, NetworkManager, wg-quick, OpenVPN, macOS`

---

### Task 7: `vpns init` with detection

**Files:**
- Modify: `src/cli.rs` (`Init { force, example }`), `src/main.rs` (`init`), `src/config.rs`, `src/ui.rs`
- Test: unit tests in `src/config.rs`, `tests/cli.rs`

**Interfaces:**
- Consumes: `detect::{detect, dedupe, render, Preset, Sources}`.
- Produces:
  - `Command::Init { force: bool, #[arg(long)] example: bool }` — doc "Write the commented example instead of detecting installed VPNs".
  - `config::append_presets(existing: &str, blocks: &[String]) -> Result<String, ConfigError>`: validates `existing` with `Config::from_toml` first (propagate its error), returns `existing` + (`"\n"` if it does not end with one) + `"\n"` + blocks joined by `"\n"`.
  - `config::names_in(existing: &str) -> Vec<String>`: names of a valid config (for `dedupe`'s `taken` and the "already configured" filter).
  - `ui::select_presets(presets: &[Preset]) -> anyhow::Result<Option<Vec<usize>>>`: inquire `MultiSelect`, title `Found N VPN profiles:`, all checked (`with_all_selected_by_default`), labels `name  source` aligned, plus ` · unverified` (dimmed) when `!verified` and ` · template` when `template`; help `space toggle · enter write · esc cancel`; `None` on Esc/Ctrl-C (reuse `picker_outcome` mapping; `NotTTY` → error).

Flow of `init(path, force, example)`:
1. `example`, or not a TTY (`ui::ensure_tty().is_err()`) → today's behavior unchanged (example; error if exists unless `--force`).
2. Else `detect(&Sources::system(&ShellRunner))`; empty → print `No VPN client detected; wrote the commented example instead.` and do (1).
3. If the file exists and not `force`: `existing = read`, `taken = names_in(existing)?` (invalid → error `{path} is not valid; fix it or rerun with --force` with the parse error as context); drop presets whose `name` is in `taken` ("already configured"), then `dedupe(presets, &taken)` (only collisions inside the detected list remain to suffix). If nothing left → `Nothing new to add: every detected VPN is already in {path}.`, exit 0.
4. `select_presets`; `None` → print `Cancelled.`, exit 0, nothing written. Empty selection → `Nothing selected.`, exit 0.
5. Write: new/forced file → header comment of `EXAMPLE` (its comment lines only) + blocks; existing → `append_presets`. Print `Wrote {path} ({n} VPNs). Check it with `vpns config check`.` (`Added … to` when appending).

- [ ] **Step 1: Tests**
  - `append_preserves_existing_bytes` (Review Focus 2): existing = `"# mine\n[[vpn]]\nname = \"a\"\nup = \"u\"\ndown = \"d\""` (no trailing newline) → result starts with the exact existing string, parses, has 2 VPNs.
  - `append_refuses_invalid_existing` (Review Focus 3): existing with duplicate names → `Err(DuplicateName)`.
  - `names_in_lists_names`.
  - CLI: existing test `init_writes_example_and_refuses_overwrite` must still pass unchanged (assert_cmd has no TTY); add `init_example_flag_skips_detection` (`init --example` writes a file containing `protonvpn connect`).
- [ ] **Step 2: Run, expect FAIL.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run all tests, PASS.** Manual check in a real terminal against a temp file: `cargo run -- -c "$(mktemp -d)/c.toml" init`, Esc → `Cancelled.` and no file; run again, Enter → file written, then `cargo run -- -c <same> config check` (status probes only — read-only). Run `init` a third time → `Nothing new to add`.
- [ ] **Step 5: Commit** — `feat: vpns init detects installed VPNs and their profiles`

---

### Task 8: Release pipeline

**Files:**
- Create: `.github/scripts/package.sh`, `.github/scripts/check-version.sh`
- Modify: `.github/workflows/release.yml`

**Interfaces:**
- `package.sh <target> <bin> <gen-bin>`: creates `vpns-<target>/` with `<bin>` as `vpns`, `README.md`, `LICENSE`, `CHANGELOG.md`, `completions/{vpns.bash,_vpns,vpns.fish}` and `vpns.1` (generated by running `<gen-bin> completions …` / `<gen-bin> man`); writes `vpns-<target>.tar.gz` and `vpns-<target>.tar.gz.sha256` (`sha256sum`, else `shasum -a 256`); prints the archive path.
- `check-version.sh <tag>`: fails unless `${tag#v}` equals the `version` in `Cargo.toml` (first `^version = "…"` line, read with `sed`) and the CHANGELOG awk extraction (moved here from the workflow) is non-empty; writes `notes.md`.

- [ ] **Step 1: Test the scripts locally**

Run: `cargo build --release && .github/scripts/package.sh x86_64-unknown-linux-gnu target/release/vpns target/release/vpns && tar tzf vpns-x86_64-unknown-linux-gnu.tar.gz && (cd . && sha256sum -c vpns-x86_64-unknown-linux-gnu.tar.gz.sha256)`
Expected: the listing shows `vpns-x86_64-unknown-linux-gnu/vpns`, `…/completions/_vpns`, `…/vpns.1`; checksum `OK`. Clean up the outputs.
Run: `.github/scripts/check-version.sh v0.2.0` → success, `notes.md` non-empty; `check-version.sh v9.9.9` → failure with `::error::`. `shellcheck .github/scripts/*.sh` → clean.

- [ ] **Step 2: Rewrite `release.yml`**

- Triggers: `push: tags: ["v*"]` and `pull_request: paths: [".github/workflows/release.yml", ".github/scripts/**", "Cargo.toml", "Cargo.lock"]` (dry run).
- `permissions: {}` at top; per job as listed below.
- `draft` (tag only, `contents: write`): `check-version.sh "$GITHUB_REF_NAME"`, then the existing `gh release create … --draft --verify-tag --notes-file notes.md`.
- `build` (needs draft when tagged — use `needs: draft` + `if: ${{ !failure() && !cancelled() }}`, draft being skipped on PRs): matrix

  | target | os | test | gen-bin |
  |---|---|---|---|
  | x86_64-unknown-linux-musl | ubuntu-latest | yes | target binary |
  | aarch64-unknown-linux-musl | ubuntu-24.04-arm | yes | target binary |
  | aarch64-apple-darwin | macos-latest | yes | target binary |
  | x86_64-apple-darwin | macos-latest | no | `cargo build --release --locked` host binary |

  Steps: checkout, `dtolnay/rust-toolchain@stable` with `targets`, `Swatinem/rust-cache@v2` with `key: ${{ matrix.target }}`, `cargo test --locked` (if test), `cargo build --release --locked --target …`, `package.sh`. Tag: `actions/attest-build-provenance@v3` on the `.tar.gz` (job permissions `contents: write`, `id-token: write`, `attestations: write`), then `gh release upload … --clobber`. PR: `actions/upload-artifact@v4` instead.
- `publish` (tag only, `contents: write`): if the tag contains `-` → `--prerelease`, else `--latest`; plus `--draft=false`.
- `crates-io` (tag only, needs publish, `id-token: write`, `if: vars.CRATES_IO_TRUSTED_PUBLISHING == 'true'`): if `cargo info "vpn-switcher@${GITHUB_REF_NAME#v}" --registry crates-io` succeeds → notice "already published", exit 0; else `rust-lang/crates-io-auth-action@v1` → `cargo publish --locked` with `CARGO_REGISTRY_TOKEN` from its output.
- [ ] **Step 3: `actionlint`** → no output.
- [ ] **Step 4: Commit and push**; the PR triggers the dry run. Expected: 4 build jobs green, 4 artifacts; download the macOS ones (`gh run download`) and check `tar tzf` shows the same layout. — `ci: release macOS binaries, completions, man page, attestations and crates.io publishing`

---

### Task 9: Demo GIF pipeline

**Files:**
- Create: `demo/config.toml`, `demo/demo.tape`, `.github/workflows/demo.yml`

- [ ] **Step 1: `demo/config.toml`** — fake VPNs `acme`, `globex`, `tailscale`, each `up = "sleep 0.6 && touch /tmp/vpns-demo/<n>"`, `down = "sleep 0.4 && rm -f /tmp/vpns-demo/<n>"`, `status = "test -f /tmp/vpns-demo/<n>"`; header comment explains it is a demo with no real VPN.
Run: `mkdir -p /tmp/vpns-demo && cargo run -q -- -c demo/config.toml config check` → `3 VPNs, valid`, all `down`.
- [ ] **Step 2: `demo/demo.tape`** — `Output demo/demo.gif`, width 900 / height 420, font size 18, a dark theme; hidden setup (`Hide` … `Show`) that creates `/tmp/vpns-demo`, touches `globex`, exports `VPNS_CONFIG=demo/config.toml`, clears; then `vpns status` → `vpns` picker, `Down` to `acme`, `Enter` → `vpns config check`. Under 20 s total.
- [ ] **Step 3: `demo.yml`** — `workflow_dispatch` only; permissions `contents: write`, `pull-requests: write`; build `vpns` (`cargo build --release`, add `target/release` to `PATH`), `charmbracelet/vhs-action@v2` with `path: demo/demo.tape`, then `peter-evans/create-pull-request@v7` (branch `demo-gif`, title `docs: regenerate the demo GIF`). `actionlint` clean.
- [ ] **Step 4: Commit** — `docs: vhs demo and workflow to render it`

---

### Task 10: README, SECURITY, RELEASING, CHANGELOG, version 0.3.0

**Files:**
- Modify: `README.md`, `CHANGELOG.md`, `Cargo.toml` (`version = "0.3.0"`), `Cargo.lock`
- Create: `SECURITY.md`, `RELEASING.md`

- [ ] **Step 1: README** following the spec's section list (title + tagline + GIF via `https://raw.githubusercontent.com/AdrienGras/vpn-switcher/main/demo/demo.gif`; badges crates.io version/downloads, CI, MSRV `img.shields.io/crates/msrv/vpn-switcher`, license; Why; Install (`cargo binstall vpn-switcher`, `cargo install vpn-switcher` with Rust 1.87+, archives for the 4 targets, `gh attestation verify vpns-<target>.tar.gz --repo AdrienGras/vpn-switcher`, completions install for bash/zsh/fish from `vpns completions`), Quick start, Usage (+ `config`, `completions`), Presets (one block per detector of Task 6, verified ones flagged), Configuration (existing reference), FAQ, Platforms (Linux, macOS; Windows not supported), License). Every repo link absolute (`https://github.com/AdrienGras/vpn-switcher/...`).
Check: `grep -nE '\]\((\./|[a-zA-Z_-]+\.(md|toml|gif))' README.md` → no output.
- [ ] **Step 2: SECURITY.md** — report privately via GitHub's "Report a vulnerability" (Security tab); supported version = latest release.
- [ ] **Step 3: RELEASING.md** — the release procedure and the first-release runbook from the spec ("Actions manuelles d'Adrien", in order), in English, plus the final order (merge PR → dispatch `demo.yml` → merge GIF PR → tag) and the repo setting "Allow GitHub Actions to create and approve pull requests" needed by `demo.yml`.
- [ ] **Step 4: CHANGELOG** `## [0.3.0] - <release date>` hand-written in the existing style: Added (detection in `init`, `--example`, `config path|edit|check`, completions, man page, macOS binaries, crates.io/binstall, attestations), Changed (MSRV 1.87, library hidden), plus the compare link.
- [ ] **Step 5: Bump to 0.3.0** and run `cargo build` (updates `Cargo.lock`), `.github/scripts/check-version.sh v0.3.0` → success, `cargo package --list` unchanged in shape, `cargo publish --dry-run --locked` → success (no upload).
- [ ] **Step 6: Full verification** — `cargo fmt --all --check`, `cargo clippy --all-targets --locked -- -D warnings`, `rtk proxy cargo test`, `cargo +1.87 test --locked`, `cargo deny check`, `actionlint`. All clean.
- [ ] **Step 7: Commit, push, mark the PR ready** — `docs: README, security policy, release procedure and changelog for 0.3.0`; `gh pr ready`. CI must be green on every job.

---

## After the plan (not tasks — the user does or approves these)

1. Whole-branch review (Opus), fixes, then Adrien merges the PR.
2. Dispatch `demo.yml`, merge the GIF PR.
3. Tag `v0.3.0` and push it (ask Adrien first).
4. Adrien's manual steps from `RELEASING.md` (first `cargo publish`, trusted publisher, `CRATES_IO_TRUSTED_PUBLISHING` variable, revoke token, private vulnerability reporting).
5. Verify: `cargo binstall vpn-switcher --root "$(mktemp -d)" --no-confirm` installs 0.3.0 from the archive.
