# vpn-switcher Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A polished Rust CLI `vpns` that shows the state of several mutually incompatible VPNs and switches between them (interactive arrow-key picker, or subcommands).

**Architecture:** A binary crate split by responsibility: config loading/validation, a `Runner` trait that executes shell commands (real impl + test fakes), pure state classification, pure switch planning + execution, and a thin UI layer (`inquire` picker, colored table, `indicatif` spinner) wired by `clap`.

**Tech Stack:** Rust 1.99 (edition 2024), clap (derive), serde + toml, etcetera, regex, thiserror, anyhow, inquire, owo-colors, indicatif, wait-timeout. Dev: assert_cmd, predicates, tempfile.

**Spec:** `docs/superpowers/specs/2026-11-06-vpn-switcher-design.md`

## Global Constraints

- Binary name `vpns`, crate/repo name `vpn-switcher`, `edition = "2024"`, `rust-version = "1.99"`, license MIT.
- Add every dependency with `cargo add` (latest version); no `unwrap()`/`expect()` outside tests.
- Config path: `$XDG_CONFIG_HOME/vpn-switcher/config.toml` (fallback `~/.config/...`, via `etcetera::choose_base_strategy()`), overridable by `--config <PATH>` and env `VPNS_CONFIG` (flag wins).
- Status rule: `up_pattern` (regex over stdout+stderr) wins if set; else exit code ∈ `up_codes` (default `[0]`) → Up, ∈ `down_codes` (default `[1]`) → Down, else Unknown. No `status` command → Unknown.
- All commands run through `sh -c`. Up/down commands inherit stdin/stdout/stderr (so `sudo` can prompt) and must run while no spinner/picker owns the terminal. Status probes get stdin = null and a 10 s timeout.
- User-facing output in English; `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test` must pass at every commit.

## Review Focus

1. A `status` command that hangs (e.g. VPN daemon stuck) → probe times out after 10 s, state Unknown, CLI stays responsive. Test in Task 2.
2. Bad config (invalid regex in `up_pattern`, duplicate names, zero VPNs, unknown keys) → error naming the VPN/field, no panic. Tests in Task 1.
3. Esc / Ctrl-C in the picker → nothing executed, exit code 0 with "Cancelled". Covered by `picker_outcome` mapping test in Task 4.
4. A down or up command fails mid-switch → execution stops right there, the failure is reported with the VPN name, `vpns` exits non-zero. Test in Task 3 + CLI test in Task 4.
5. `vpns` run without a TTY (piped/cron) or `vpns up <unknown>` → clear error pointing to `vpns up <name>` / listing known names, exit non-zero. CLI tests in Task 4.

---

### Task 1: Scaffold + configuration

**Files:**
- Create: `Cargo.toml`, `src/main.rs` (stub: `mod config; fn main() {}` for now), `src/config.rs`, `.gitignore` (`/target`)

**Interfaces:**
- Produces:
  - `pub struct Vpn { pub name: String, pub up: String, pub down: String, pub status: Option<String>, pub up_pattern: Option<regex::Regex>, pub up_codes: Vec<i32>, pub down_codes: Vec<i32> }`
  - `pub struct Config { pub vpns: Vec<Vpn> }` with `impl Config { pub fn from_toml(s: &str) -> Result<Config, ConfigError>; pub fn load(path: &Path) -> Result<Config, ConfigError>; pub fn find(&self, name: &str) -> Option<usize> }`
  - `pub fn default_path() -> Result<PathBuf, ConfigError>` and `pub fn resolve_path(flag: Option<PathBuf>) -> Result<PathBuf, ConfigError>` (flag > `VPNS_CONFIG` > default)
  - `pub const EXAMPLE: &str` — the example config (proton / tailscale / netbird) written by `vpns init`
  - `#[derive(thiserror::Error, Debug)] pub enum ConfigError { NotFound(PathBuf), Io{path, source}, Parse(toml::de::Error), Empty, DuplicateName(String), InvalidPattern{name: String, source: regex::Error}, NoConfigDir }`
- TOML is deserialized into a private `RawConfig { #[serde(rename = "vpn", default)] vpn: Vec<RawVpn> }` with `#[serde(deny_unknown_fields)]`, then validated into `Config`.

- [ ] **Step 1: `cargo init --name vpn-switcher`**, set `[[bin]] name = "vpns"`, edition/rust-version/license/description/repository (`https://github.com/AdrienGras/vpn-switcher`), add deps listed in Tech Stack.
- [ ] **Step 2: Write failing tests in `src/config.rs`** (`#[cfg(test)] mod tests`):
  - `parses_minimal_vpn_with_default_codes`: `[[vpn]] name="a" up="u" down="d"` → `status == None`, `up_codes == [0]`, `down_codes == [1]`, `up_pattern.is_none()`.
  - `parses_pattern_and_custom_codes`: `up_pattern = "Status: Connected"`, `up_codes=[0,2]` → regex matches `"Status: Connected"`, codes kept.
  - `rejects_invalid_pattern`: `up_pattern = "("` → `ConfigError::InvalidPattern { name == "a", .. }`.
  - `rejects_duplicate_names`, `rejects_empty_config` (`""` → `Empty`), `rejects_unknown_key` (`foo = 1` in a vpn → `Parse`).
  - `example_config_is_valid`: `Config::from_toml(EXAMPLE)` → 3 VPNs named `proton`, `tailscale`, `netbird`.
  - `find_returns_index`.
  - `load_missing_file_is_not_found`: path in a `tempfile::tempdir()` → `NotFound`.
- [ ] **Step 3: Run `cargo test config`** → FAIL (missing items).
- [ ] **Step 4: Implement `src/config.rs`.** `EXAMPLE` content:
  ```toml
  # vpn-switcher configuration
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
- [ ] **Step 5: `cargo test config`** → PASS; fmt + clippy clean.
- [ ] **Step 6: Commit** `feat: configuration loading and validation`.

### Task 2: Command runner + state probing

**Files:**
- Create: `src/runner.rs`, `src/state.rs`; Modify: `src/main.rs` (add `mod`s)

**Interfaces:**
- Consumes: `config::Vpn`.
- Produces:
  - `src/runner.rs`: `pub enum Probe { Exited { code: Option<i32>, output: String }, TimedOut, Failed(String) }`; `pub trait Runner: Sync { fn probe(&self, cmd: &str, timeout: Duration) -> Probe; fn run(&self, cmd: &str) -> std::io::Result<bool>; }` (`run` = interactive, inherited stdio, returns `status.success()`); `pub struct ShellRunner;` implementing it; `pub const STATUS_TIMEOUT: Duration = Duration::from_secs(10);`
  - `src/state.rs`: `#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum VpnState { Up, Down, Unknown }`; `pub fn classify(vpn: &Vpn, probe: &Probe) -> VpnState`; `pub fn probe_all(vpns: &[Vpn], runner: &dyn Runner) -> Vec<VpnState>` (one `std::thread::scope` thread per VPN, results in config order).
- `ShellRunner::probe`: `sh -c cmd`, stdin null, stdout+stderr piped and drained by reader threads (avoid pipe deadlock), `wait_timeout::ChildExt::wait_timeout`; on timeout kill + wait → `TimedOut`; spawn error → `Failed(msg)`. Output = stdout followed by stderr.

- [ ] **Step 1: Write failing tests.**
  - `state.rs`: `pattern_wins_over_code` (pattern set, output contains it, code 1 → Up; output lacks it, code 0 → Down); `default_codes` (0 → Up, 1 → Down, 3 → Unknown); `custom_codes` (`up_codes=[5]`, code 5 → Up); `no_status_command_is_unknown`; `timed_out_and_failed_are_unknown`; `probe_all_keeps_order` using a fake `Runner` returning per-command `Probe`s.
  - `runner.rs`: `probe_captures_output_and_code` (`"echo hi; echo err >&2; exit 3"` → `Exited { code: Some(3), output }` with output containing `hi` and `err`); `probe_times_out` (`"sleep 5"`, timeout 200 ms → `TimedOut`, test finishes < 2 s); `run_reports_success` (`"true"` → `Ok(true)`, `"false"` → `Ok(false)`).
- [ ] **Step 2: `cargo test`** → FAIL.
- [ ] **Step 3: Implement** both modules (`classify` returns Unknown when `vpn.status` is `None` without looking at the probe).
- [ ] **Step 4: `cargo test`** → PASS; fmt + clippy clean.
- [ ] **Step 5: Commit** `feat: command runner and VPN state probing`.

### Task 3: Switch planning and execution

**Files:**
- Create: `src/switch.rs`; Modify: `src/main.rs`

**Interfaces:**
- Consumes: `Vpn`, `VpnState`, `Runner`.
- Produces:
  - `#[derive(Debug, PartialEq, Eq, Clone, Copy)] pub enum Action { Down(usize), Up(usize) }`
  - `pub fn plan_switch(states: &[VpnState], target: Option<usize>) -> Vec<Action>` — `target: None` = all off.
  - `pub fn plan_down(states: &[VpnState], index: usize) -> Vec<Action>` — `[Down(index)]` unless that VPN is `Down`.
  - `pub fn execute(actions: &[Action], vpns: &[Vpn], runner: &dyn Runner, mut on_step: impl FnMut(Step)) -> Result<(), SwitchError>`
  - `pub enum Step<'a> { Starting(Action, &'a str), Done(Action, &'a str) }`
  - `#[derive(thiserror::Error, Debug)] pub enum SwitchError { #[error("`{name}` {verb} command failed")] CommandFailed { name: String, verb: &'static str }, #[error(...)] Spawn { name: String, source: std::io::Error } }` (verb = `"down"`/`"up"`)
- Planning rule (decided, not in spec): every VPN other than the target whose state is `Up` **or `Unknown`** gets `Down` (we cannot prove it is off; anti-goal is two VPNs up), in config order; then `Up(target)` unless target is already `Up`. Execution stops at the first failing action.

- [ ] **Step 1: Write failing tests:**
  - `switch_downs_active_then_ups_target`: states `[Up, Down, Down]`, target `Some(2)` → `[Down(0), Up(2)]`.
  - `switch_downs_unknown_too`: `[Unknown, Down, Up]`, target `Some(1)` → `[Down(0), Down(2), Up(1)]`.
  - `target_already_up_only_cleans_others`: `[Up, Up]`, `Some(0)` → `[Down(1)]`; `[Up, Down]`, `Some(0)` → `[]`.
  - `all_off`: `[Up, Down, Unknown]`, `None` → `[Down(0), Down(2)]`.
  - `plan_down_skips_down_vpn`.
  - `execute_stops_at_first_failure`: fake runner recording commands, `down` of VPN 0 fails → `Err(CommandFailed { name: "a", verb: "down" })`, `up` of target never called.
  - `execute_emits_steps_in_order`.
- [ ] **Step 2: `cargo test switch`** → FAIL.
- [ ] **Step 3: Implement** `src/switch.rs`.
- [ ] **Step 4: `cargo test`** → PASS; fmt + clippy clean.
- [ ] **Step 5: Commit** `feat: switch planning and execution`.

### Task 4: CLI and UI

**Files:**
- Create: `src/cli.rs`, `src/ui.rs`, `tests/cli.rs`; Modify: `src/main.rs`

**Interfaces:**
- Consumes: everything above.
- Produces:
  - `src/cli.rs`: clap derive `Cli { #[arg(long, global = true)] config: Option<PathBuf>, #[command(subcommand)] command: Option<Command> }`, `enum Command { Switch, Status, Up { name: String }, Down { name: String }, Off, Init { #[arg(long)] force: bool } }`. No subcommand = `Switch`. `Up` switches (downs the others). Help text and `about` polished; `--version` enabled.
  - `src/ui.rs`: `pub fn print_status(vpns: &[Vpn], states: &[VpnState])` (aligned table: colored dot `●` green Up / dim Down / yellow `?` Unknown, name bold, state label); `pub fn probe_with_spinner(vpns, runner) -> Vec<VpnState>` (indicatif spinner "Checking VPN status…", cleared when done; hidden when stderr is not a TTY); `pub enum PickerOutcome { Target(Option<usize>), Cancelled }`; `pub fn pick(vpns, states) -> anyhow::Result<PickerOutcome>` (`inquire::Select`, one line per VPN like the table + final entry `⏻  Disconnect all` → `Target(None)`; cursor starts on the first Up VPN; render config with colors); `pub fn picker_outcome(err: inquire::InquireError) -> anyhow::Result<PickerOutcome>` (`OperationCanceled`/`OperationInterrupted` → `Ok(Cancelled)`, `NotTTY` → error "interactive picker needs a terminal; use `vpns up <name>`", else error); `pub fn report_step(step: Step)` (`↓ Stopping proton…` / `↑ Starting tailscale…` before, `✓` line after; plain `println!`, no spinner, so sudo can prompt).
  - `src/main.rs`: `fn main() -> ExitCode` — runs `run(cli) -> anyhow::Result<()>`, prints errors as `error: {e:#}` in red to stderr and returns `ExitCode::FAILURE`. After any executed switch/up/down/off: re-probe and `print_status`. Empty plan → print `Nothing to do.` Missing config → error mentions the path and suggests `vpns init`. `init` writes `EXAMPLE` (creating parent dirs), refuses to overwrite without `--force`, prints the path. `up/down <name>` with unknown name → error `unknown VPN `x` (known: a, b)`.
- `tests/cli.rs` uses `assert_cmd` with `--config <tempfile>` where commands are shell snippets (`true`, `false`, `echo ...`, `touch $DIR/x`).

- [ ] **Step 1: Write failing tests:**
  - `ui.rs` unit: `picker_outcome_maps_cancel` (`OperationCanceled` and `OperationInterrupted` → `Cancelled`; `NotTTY` → `Err` whose message contains `vpns up`).
  - `tests/cli.rs`: `status_shows_each_vpn` (two VPNs, status `true` / `false` → stdout contains both names, `up`, `down`, exit 0); `up_switches_and_reports` (VPN a up (`status = "test -f $D/a"`), `down = "rm $D/a"`, b `up = "touch $D/b"` → `vpns up b` succeeds, `$D/a` gone, `$D/b` exists); `up_failure_exits_nonzero` (`up = "false"` → failure, stderr contains `up command failed`); `unknown_name_lists_known`; `missing_config_suggests_init`; `init_writes_example_and_refuses_overwrite`; `bare_vpns_without_tty_errors` (stdin not a TTY → failure, stderr contains `vpns up`); `off_downs_everything`.
- [ ] **Step 2: `cargo test`** → FAIL.
- [ ] **Step 3: Implement** `cli.rs`, `ui.rs`, `main.rs`.
- [ ] **Step 4: `cargo test`** → PASS; fmt + clippy clean.
- [ ] **Step 5: Manual smoke run** `cargo run -- --config <EXAMPLE copy> status` on this machine → table with proton/tailscale/netbird; do **not** run up/down on the real VPNs.
- [ ] **Step 6: Commit** `feat: interactive picker and CLI commands`.

### Task 5: README, CI, public GitHub repo

**Files:**
- Create: `README.md`, `LICENSE` (MIT, 2026, Adrien Gras), `.github/workflows/ci.yml`

- [ ] **Step 1: `README.md`** (English): pitch, install (`cargo install --git https://github.com/AdrienGras/vpn-switcher`), usage for each command, config reference (all keys + status rule + sudo note + `VPNS_CONFIG`), the example config.
- [ ] **Step 2: CI** on push/PR: `dtolnay/rust-toolchain@stable` with `rustfmt, clippy`, `Swatinem/rust-cache`, then `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`.
- [ ] **Step 3: Verify** fmt/clippy/test locally → all green.
- [ ] **Step 4: Commit** `docs: README, license and CI`.
- [ ] **Step 5: `gh repo create AdrienGras/vpn-switcher --public --source . --push --description "Switch between mutually incompatible VPNs from a tidy CLI"`**, then `gh run watch` is not used; check `gh run list --limit 1` shows the CI run.
