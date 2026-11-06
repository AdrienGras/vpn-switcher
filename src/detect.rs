//! Presets produced by VPN detection, and their rendering to `config.toml`.
//!
//! Rendering is pure. Detection only reads: it looks for executables in `$PATH`,
//! lists profile directories and runs read-only listing commands through the
//! [`Runner`]. It never runs a VPN up or down, and never uses `sudo`.

use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::runner::{Probe, Runner};

/// A VPN entry proposed by a detector, ready to be rendered as a `[[vpn]]` table.
#[derive(Debug, Clone, PartialEq)]
pub struct Preset {
    /// Name used by `vpns`, already sanitized.
    pub name: String,
    /// Where the preset comes from, e.g. "NetworkManager (openvpn)".
    pub source: String,
    pub up: String,
    pub down: String,
    pub status: Option<String>,
    pub up_pattern: Option<String>,
    /// `None` means the default exit codes.
    pub down_codes: Option<Vec<i32>>,
    pub verified: bool,
    /// Written fully commented out.
    pub template: bool,
    /// Extra comment line, e.g. "replace vpn.example.com with your server".
    pub note: Option<String>,
}

/// Quotes `s` for `sh` with single quotes (`it's` becomes `'it'\''s'`).
pub fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Derives a `vpns` name from a profile name: lowercase, runs of characters
/// outside `[a-z0-9._-]` collapsed to one `-`, trimmed, `vpn` if empty.
pub fn vpn_name(profile: &str) -> String {
    let mut name = String::new();
    for c in profile.to_lowercase().chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-') {
            name.push(c);
        } else if !name.ends_with('-') {
            name.push('-');
        }
    }
    let name = name.trim_matches('-');
    if name.is_empty() {
        "vpn".to_string()
    } else {
        name.to_string()
    }
}

/// Makes names unique, in order: a name already in `taken` or used earlier in
/// the list gets `-2`, `-3`, ... (first free).
pub fn dedupe(presets: Vec<Preset>, taken: &[&str]) -> Vec<Preset> {
    let mut used: std::collections::HashSet<String> = taken.iter().map(|s| s.to_string()).collect();
    presets
        .into_iter()
        .map(|mut p| {
            if used.contains(&p.name) {
                let base = p.name.clone();
                p.name = (2..)
                    .map(|n| format!("{base}-{n}"))
                    .find(|candidate| !used.contains(candidate))
                    .expect("unbounded range");
            }
            used.insert(p.name.clone());
            p
        })
        .collect()
}

fn toml_string(s: &str) -> String {
    toml::Value::String(s.to_string()).to_string()
}

/// A line break would end a `#` comment early: flatten it to a space.
fn comment_text(s: &str) -> String {
    s.replace(['\n', '\r'], " ")
}

/// Renders one `[[vpn]]` block (with its comments), ending with a newline.
pub fn render(preset: &Preset) -> String {
    let mut out = format!("# detected: {}\n", comment_text(&preset.source));
    if !preset.verified {
        out.push_str("# preset not verified on a real VPN — run `vpns config check`\n");
    }
    if let Some(note) = &preset.note {
        out.push_str(&format!("# {}\n", comment_text(note)));
    }
    let mut table = vec![
        "[[vpn]]".to_string(),
        format!("name = {}", toml_string(&preset.name)),
        format!("up = {}", toml_string(&preset.up)),
        format!("down = {}", toml_string(&preset.down)),
    ];
    if let Some(status) = &preset.status {
        table.push(format!("status = {}", toml_string(status)));
    }
    if let Some(pattern) = &preset.up_pattern {
        table.push(format!("up_pattern = {}", toml_string(pattern)));
    }
    if let Some(codes) = &preset.down_codes {
        let codes: Vec<String> = codes.iter().map(i32::to_string).collect();
        table.push(format!("down_codes = [{}]", codes.join(", ")));
    }
    // A value may encode as several lines (multi-line string): prefix each one.
    for line in table.iter().flat_map(|entry| entry.lines()) {
        if preset.template {
            out.push_str("# ");
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// How long a listing command may run before its detector is dropped.
const LIST_TIMEOUT: Duration = Duration::from_secs(5);

/// Everything detection looks at, injectable for tests.
pub struct Sources<'a> {
    /// `std::env::consts::OS` in production: "linux" or "macos".
    pub os: &'a str,
    /// `$PATH`.
    pub path: OsString,
    pub wireguard_dir: PathBuf,
    pub openvpn_dir: PathBuf,
    pub tailscale_app: PathBuf,
    pub runner: &'a dyn Runner,
}

impl Sources<'_> {
    pub fn system(runner: &dyn Runner) -> Sources<'_> {
        Sources {
            os: std::env::consts::OS,
            path: std::env::var_os("PATH").unwrap_or_default(),
            wireguard_dir: PathBuf::from("/etc/wireguard"),
            openvpn_dir: PathBuf::from("/etc/openvpn/client"),
            tailscale_app: PathBuf::from("/Applications/Tailscale.app/Contents/MacOS/Tailscale"),
            runner,
        }
    }

    fn linux(&self) -> bool {
        self.os == "linux"
    }

    fn macos(&self) -> bool {
        self.os == "macos"
    }

    /// True if an executable file called `bin` sits in one of the `$PATH` entries.
    fn has_bin(&self, bin: &str) -> bool {
        std::env::split_paths(&self.path)
            // An empty entry would mean the current directory: skip it.
            .filter(|dir| !dir.as_os_str().is_empty())
            .any(|dir| is_executable(&dir.join(bin)))
    }

    /// Output of a read-only listing command; `None` unless it exited with 0.
    fn list(&self, cmd: &str) -> Option<String> {
        match self.runner.probe(cmd, LIST_TIMEOUT) {
            Probe::Exited {
                code: Some(0),
                output,
            } => Some(output),
            _ => None,
        }
    }
}

fn is_executable(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

/// Detects the VPNs installed on this machine, in a fixed order, not yet deduped.
pub fn detect(src: &Sources) -> Vec<Preset> {
    let mut presets = Vec::new();
    presets.extend(protonvpn(src));
    presets.extend(tailscale(src));
    presets.extend(netbird(src));
    presets.extend(nmcli(src));
    presets.extend(wg_quick(src));
    presets.extend(openvpn(src));
    presets.extend(openconnect(src));
    presets.extend(openfortivpn(src));
    presets.extend(mullvad(src));
    presets.extend(warp_cli(src));
    presets.extend(scutil(src));
    presets
}

fn preset(name: &str, source: &str, verified: bool) -> Preset {
    Preset {
        name: name.to_string(),
        source: source.to_string(),
        up: String::new(),
        down: String::new(),
        status: None,
        up_pattern: None,
        down_codes: None,
        verified,
        template: false,
        note: None,
    }
}

/// A vendor CLI with `<bin> <up> / <down> / <status>` style commands.
fn vendor(
    name: &str,
    source: &str,
    verified: bool,
    cmds: [&str; 3],
    up_pattern: Option<&str>,
) -> Preset {
    let mut p = preset(name, source, verified);
    p.up = cmds[0].to_string();
    p.down = cmds[1].to_string();
    p.status = Some(cmds[2].to_string());
    p.up_pattern = up_pattern.map(str::to_string);
    p
}

fn protonvpn(src: &Sources) -> Vec<Preset> {
    if !(src.linux() && src.has_bin("protonvpn")) {
        return Vec::new();
    }
    vec![vendor(
        "proton",
        "protonvpn",
        true,
        [
            "protonvpn connect",
            "protonvpn disconnect",
            "protonvpn status",
        ],
        Some("Status: Connected"),
    )]
}

fn tailscale(src: &Sources) -> Vec<Preset> {
    // On macOS the app bundle's binary is used by absolute path when `tailscale` is not in PATH.
    let bin = if src.has_bin("tailscale") && (src.linux() || src.macos()) {
        "tailscale".to_string()
    } else if src.macos() && is_executable(&src.tailscale_app) {
        shell_quote(&src.tailscale_app.to_string_lossy())
    } else {
        return Vec::new();
    };
    vec![vendor(
        "tailscale",
        "tailscale",
        src.linux(),
        [
            &format!("{bin} up"),
            &format!("{bin} down"),
            &format!("{bin} status"),
        ],
        None,
    )]
}

fn netbird(src: &Sources) -> Vec<Preset> {
    if !((src.linux() || src.macos()) && src.has_bin("netbird")) {
        return Vec::new();
    }
    vec![vendor(
        "netbird",
        "netbird",
        src.linux(),
        ["netbird up", "netbird down", "netbird status"],
        Some("Management: Connected"),
    )]
}

/// Splits `nmcli -t` output into `(name, type)`; `:` and `\` in names are backslash-escaped.
fn parse_nmcli(output: &str) -> Vec<(String, String)> {
    output
        .lines()
        .filter_map(|line| {
            let (name, kind) = line.rsplit_once(':')?;
            let mut unescaped = String::new();
            let mut chars = name.chars();
            while let Some(c) = chars.next() {
                if c == '\\' {
                    match chars.next() {
                        Some(next @ (':' | '\\')) => unescaped.push(next),
                        Some(other) => {
                            unescaped.push(c);
                            unescaped.push(other);
                        }
                        None => unescaped.push(c),
                    }
                } else {
                    unescaped.push(c);
                }
            }
            Some((unescaped, kind.to_string()))
        })
        .collect()
}

fn nmcli(src: &Sources) -> Vec<Preset> {
    if !(src.linux() && src.has_bin("nmcli")) {
        return Vec::new();
    }
    let Some(output) = src.list("nmcli -t -f NAME,TYPE connection show") else {
        return Vec::new();
    };
    parse_nmcli(&output)
        .into_iter()
        .filter(|(_, kind)| kind == "vpn" || kind == "wireguard")
        .map(|(profile, kind)| {
            let q = shell_quote(&profile);
            vendor(
                &vpn_name(&profile),
                &format!("NetworkManager ({kind})"),
                false,
                [
                    &format!("nmcli connection up id {q}"),
                    &format!("nmcli connection down id {q}"),
                    &format!("nmcli -g GENERAL.STATE connection show id {q}"),
                ],
                Some("(?m)^activated$"),
            )
        })
        .collect()
}

/// `*.conf` file stems of `dir`, sorted. `None` if the directory is unreadable.
fn conf_profiles(dir: &Path) -> Option<Vec<String>> {
    let mut stems: Vec<String> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "conf") && path.is_file())
        .filter_map(|path| Some(path.file_stem()?.to_str()?.to_string()))
        .collect();
    stems.sort();
    Some(stems)
}

fn wg_quick(src: &Sources) -> Vec<Preset> {
    if !(src.linux() && src.has_bin("wg-quick")) {
        return Vec::new();
    }
    let source = format!("wg-quick ({})", src.wireguard_dir.display());
    match conf_profiles(&src.wireguard_dir).filter(|profiles| !profiles.is_empty()) {
        Some(profiles) => profiles
            .iter()
            .map(|n| {
                let q = shell_quote(n);
                vendor(
                    &vpn_name(n),
                    &source,
                    false,
                    [
                        &format!("sudo wg-quick up {q}"),
                        &format!("sudo wg-quick down {q}"),
                        &format!("ip link show {q}"),
                    ],
                    None,
                )
            })
            .collect(),
        None => {
            let mut p = vendor(
                "wg-quick",
                &source,
                false,
                [
                    "sudo wg-quick up NAME",
                    "sudo wg-quick down NAME",
                    "ip link show NAME",
                ],
                None,
            );
            p.template = true;
            p.note = Some(format!(
                "no profile found: put NAME.conf files in {} and replace NAME",
                src.wireguard_dir.display()
            ));
            vec![p]
        }
    }
}

fn openvpn(src: &Sources) -> Vec<Preset> {
    if !(src.linux() && src.has_bin("openvpn")) {
        return Vec::new();
    }
    let source = format!("OpenVPN ({})", src.openvpn_dir.display());
    let (mut presets, down_codes) = (Vec::new(), Some(vec![3]));
    match conf_profiles(&src.openvpn_dir).filter(|profiles| !profiles.is_empty()) {
        Some(profiles) => {
            for n in profiles {
                let unit = shell_quote(&format!("openvpn-client@{n}"));
                let mut p = vendor(
                    &vpn_name(&n),
                    &source,
                    false,
                    [
                        &format!("sudo systemctl start {unit}"),
                        &format!("sudo systemctl stop {unit}"),
                        &format!("systemctl is-active --quiet {unit}"),
                    ],
                    None,
                );
                p.down_codes = down_codes.clone();
                presets.push(p);
            }
        }
        None => {
            let mut p = vendor(
                "openvpn",
                &source,
                false,
                [
                    "sudo systemctl start openvpn-client@NAME",
                    "sudo systemctl stop openvpn-client@NAME",
                    "systemctl is-active --quiet openvpn-client@NAME",
                ],
                None,
            );
            p.down_codes = down_codes;
            p.template = true;
            p.note = Some(format!(
                "no profile found: put NAME.conf files in {} and replace NAME",
                src.openvpn_dir.display()
            ));
            presets.push(p);
        }
    }
    presets
}

fn template(name: &str, cmds: [&str; 3], note: &str) -> Preset {
    let mut p = vendor(name, name, false, cmds, None);
    p.template = true;
    p.note = Some(note.to_string());
    p
}

fn openconnect(src: &Sources) -> Vec<Preset> {
    if !((src.linux() || src.macos()) && src.has_bin("openconnect")) {
        return Vec::new();
    }
    vec![template(
        "openconnect",
        [
            "sudo openconnect --background --protocol=anyconnect vpn.example.com",
            "sudo pkill -x openconnect",
            "pgrep -x openconnect",
        ],
        "replace vpn.example.com; use --protocol=gp for GlobalProtect",
    )]
}

fn openfortivpn(src: &Sources) -> Vec<Preset> {
    if !((src.linux() || src.macos()) && src.has_bin("openfortivpn")) {
        return Vec::new();
    }
    vec![template(
        "openfortivpn",
        [
            "sudo sh -c 'openfortivpn vpn.example.com:443 >/dev/null 2>&1 &'",
            "sudo pkill -x openfortivpn",
            "pgrep -x openfortivpn",
        ],
        "replace vpn.example.com:443, or use -c /etc/openfortivpn/config",
    )]
}

fn mullvad(src: &Sources) -> Vec<Preset> {
    if !((src.linux() || src.macos()) && src.has_bin("mullvad")) {
        return Vec::new();
    }
    vec![vendor(
        "mullvad",
        "mullvad",
        false,
        ["mullvad connect", "mullvad disconnect", "mullvad status"],
        Some("^Connected"),
    )]
}

fn warp_cli(src: &Sources) -> Vec<Preset> {
    if !((src.linux() || src.macos()) && src.has_bin("warp-cli")) {
        return Vec::new();
    }
    // The `up_pattern` is unconfirmed: Cloudflare's documentation could not be checked
    // when this preset was written, hence `verified = false`.
    vec![vendor(
        "warp",
        "warp-cli",
        false,
        ["warp-cli connect", "warp-cli disconnect", "warp-cli status"],
        Some("Status update: Connected"),
    )]
}

/// Service names from `scutil --nc list`: the first quoted string of each `*` line.
fn parse_scutil(output: &str) -> Vec<String> {
    output
        .lines()
        .filter(|line| line.starts_with('*'))
        .filter_map(|line| {
            let rest = &line[line.find('"')? + 1..];
            Some(rest[..rest.find('"')?].to_string())
        })
        .collect()
}

fn scutil(src: &Sources) -> Vec<Preset> {
    if !(src.macos() && src.has_bin("scutil")) {
        return Vec::new();
    }
    let Some(output) = src.list("scutil --nc list") else {
        return Vec::new();
    };
    parse_scutil(&output)
        .into_iter()
        .map(|service| {
            let q = shell_quote(&service);
            vendor(
                &vpn_name(&service),
                "macOS network services (scutil)",
                false,
                [
                    &format!("scutil --nc start {q}"),
                    &format!("scutil --nc stop {q}"),
                    &format!("scutil --nc status {q}"),
                ],
                Some("^Connected"),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, ConfigError};

    fn preset(name: &str) -> Preset {
        Preset {
            name: name.to_string(),
            source: "test".to_string(),
            up: "up".to_string(),
            down: "down".to_string(),
            status: None,
            up_pattern: None,
            down_codes: None,
            verified: true,
            template: false,
            note: None,
        }
    }

    #[test]
    fn shell_quote_escapes_single_quotes() {
        assert_eq!(shell_quote("l'agence"), "'l'\\''agence'");
        assert_eq!(shell_quote(""), "''");
    }

    #[test]
    fn vpn_name_examples() {
        assert_eq!(vpn_name("ACME (Paris) — l'agence"), "acme-paris-l-agence");
        assert_eq!(vpn_name("  "), "vpn");
        assert_eq!(vpn_name("globex.wg"), "globex.wg");
    }

    #[test]
    fn dedupe_suffixes_collisions() {
        let presets = vec![preset("tailscale"), preset("tailscale"), preset("acme")];
        let names: Vec<String> = dedupe(presets, &["acme"])
            .into_iter()
            .map(|p| p.name)
            .collect();
        assert_eq!(names, ["tailscale", "tailscale-2", "acme-2"]);
    }

    #[test]
    fn preset_with_hostile_name_parses_and_quotes() {
        let profile = "ACME (Paris) — l'agence";
        let quoted = shell_quote(profile);
        let mut p = preset(&vpn_name(profile));
        p.up = format!("nmcli connection up id {quoted}");
        p.status = Some("it's \"x\" \\ é".to_string());
        let config = Config::from_toml(&render(&p)).expect("renders valid TOML");
        assert_eq!(config.vpns[0].up, p.up);
        assert_eq!(config.vpns[0].status, p.status);
        let out = std::process::Command::new("sh")
            .arg("-c")
            .arg(format!("printf %s {quoted}"))
            .output()
            .unwrap();
        assert_eq!(String::from_utf8(out.stdout).unwrap(), profile);
    }

    #[test]
    fn unverified_preset_is_marked() {
        let mut p = preset("a");
        assert!(!render(&p).contains("preset not verified"));
        p.verified = false;
        assert!(render(&p).contains("preset not verified"));
    }

    #[test]
    fn template_is_fully_commented() {
        let mut p = preset("a");
        p.template = true;
        p.verified = false;
        p.note = Some("replace vpn.example.com with your server".to_string());
        p.status = Some("s".to_string());
        p.up_pattern = Some("x".to_string());
        p.down_codes = Some(vec![3]);
        let out = render(&p);
        assert!(matches!(Config::from_toml(&out), Err(ConfigError::Empty)));
        assert!(out.lines().all(|l| l.is_empty() || l.starts_with('#')));
        assert!(out.contains("# [[vpn]]"));
    }

    #[test]
    fn render_writes_down_codes() {
        let mut p = preset("a");
        p.down_codes = Some(vec![3]);
        let config = Config::from_toml(&render(&p)).unwrap();
        assert_eq!(config.vpns[0].down_codes, [3]);
    }

    #[test]
    fn newline_in_source_or_note_cannot_end_the_comment() {
        let mut p = preset("a");
        p.source = "evil\n[[vpn]]\r".to_string();
        p.note = Some("x\r\nname = \"y\"".to_string());
        let out = render(&p);
        assert!(out.lines().take(2).all(|l| l.starts_with('#')));
        assert_eq!(Config::from_toml(&out).unwrap().vpns.len(), 1);
    }
    use crate::config::EXAMPLE;
    use crate::runner::ShellRunner;
    use std::collections::HashMap;

    struct FakeRunner(HashMap<String, Probe>);

    impl FakeRunner {
        fn new(listings: &[(&str, Probe)]) -> Self {
            Self(
                listings
                    .iter()
                    .map(|(cmd, probe)| (cmd.to_string(), probe.clone()))
                    .collect(),
            )
        }

        fn ok(cmd: &str, output: &str) -> (String, Probe) {
            (
                cmd.to_string(),
                Probe::Exited {
                    code: Some(0),
                    output: output.to_string(),
                },
            )
        }
    }

    impl Runner for FakeRunner {
        fn probe(&self, cmd: &str, _timeout: Duration) -> Probe {
            self.0
                .get(cmd)
                .cloned()
                .unwrap_or_else(|| Probe::Failed(format!("unexpected command: {cmd}")))
        }

        fn run(&self, _cmd: &str) -> std::io::Result<std::process::ExitStatus> {
            unimplemented!("detection must never run a command attached to the terminal")
        }
    }

    const NMCLI: &str = "nmcli -t -f NAME,TYPE connection show";
    const SCUTIL: &str = "scutil --nc list";

    /// A temp dir holding empty executables, used as `$PATH`.
    fn bin_dir(bins: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for bin in bins {
            let path = dir.path().join(bin);
            std::fs::write(&path, "").unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        dir
    }

    fn sources<'a>(os: &'a str, bins: &tempfile::TempDir, runner: &'a dyn Runner) -> Sources<'a> {
        let missing = bins.path().join("missing");
        Sources {
            os,
            path: bins.path().as_os_str().to_owned(),
            wireguard_dir: missing.join("wireguard"),
            openvpn_dir: missing.join("openvpn"),
            tailscale_app: missing.join("Tailscale"),
            runner,
        }
    }

    fn names(presets: &[Preset]) -> Vec<&str> {
        presets.iter().map(|p| p.name.as_str()).collect()
    }

    #[test]
    fn nothing_installed_detects_nothing() {
        let bins = bin_dir(&[]);
        let runner = FakeRunner::new(&[]);
        assert!(detect(&sources("linux", &bins, &runner)).is_empty());
        assert!(detect(&sources("macos", &bins, &runner)).is_empty());
    }

    #[test]
    fn non_executable_file_is_not_a_binary() {
        let bins = bin_dir(&[]);
        std::fs::write(bins.path().join("netbird"), "").unwrap();
        let runner = FakeRunner::new(&[]);
        assert!(detect(&sources("linux", &bins, &runner)).is_empty());
    }

    #[test]
    fn linux_vendor_clis_match_example() {
        let bins = bin_dir(&["protonvpn", "tailscale", "netbird"]);
        let runner = FakeRunner::new(&[]);
        let detected = detect(&sources("linux", &bins, &runner));
        assert_eq!(names(&detected), ["proton", "tailscale", "netbird"]);
        assert!(detected.iter().all(|p| p.verified && !p.template));

        let rendered: String = detected.iter().map(render).collect();
        let from_presets = Config::from_toml(&rendered).unwrap();
        let from_example = Config::from_toml(EXAMPLE).unwrap();
        for (a, b) in from_presets.vpns.iter().zip(&from_example.vpns) {
            assert_eq!(a.name, b.name);
            assert_eq!(a.up, b.up);
            assert_eq!(a.down, b.down);
            assert_eq!(a.status, b.status);
            assert_eq!(
                a.up_pattern.as_ref().map(|r| r.as_str()),
                b.up_pattern.as_ref().map(|r| r.as_str())
            );
        }
        assert_eq!(from_presets.vpns.len(), from_example.vpns.len());
    }

    #[test]
    fn nmcli_lists_vpn_and_wireguard_profiles() {
        let bins = bin_dir(&["nmcli"]);
        let (cmd, probe) = FakeRunner::ok(
            NMCLI,
            "acme:vpn\nWi-Fi\\:home:802-11-wireless\nglobex wg:wireguard\n",
        );
        let runner = FakeRunner::new(&[(&cmd, probe)]);
        let detected = detect(&sources("linux", &bins, &runner));
        assert_eq!(names(&detected), ["acme", "globex-wg"]);
        assert_eq!(detected[0].source, "NetworkManager (vpn)");
        assert_eq!(detected[1].source, "NetworkManager (wireguard)");
        assert_eq!(detected[0].up, "nmcli connection up id 'acme'");
        assert_eq!(
            detected[1].status.as_deref(),
            Some("nmcli -g GENERAL.STATE connection show id 'globex wg'")
        );
        assert_eq!(detected[0].up_pattern.as_deref(), Some("(?m)^activated$"));
        assert!(detected.iter().all(|p| !p.verified && !p.template));
    }

    #[test]
    fn nmcli_unescapes_colons() {
        let bins = bin_dir(&["nmcli"]);
        let (cmd, probe) = FakeRunner::ok(NMCLI, "a\\:b:vpn\nc\\\\d:vpn\n");
        let runner = FakeRunner::new(&[(&cmd, probe)]);
        let detected = detect(&sources("linux", &bins, &runner));
        assert_eq!(detected[0].up, "nmcli connection up id 'a:b'");
        assert_eq!(detected[1].up, "nmcli connection up id 'c\\d'");
    }

    #[test]
    fn failed_listing_drops_only_that_detector() {
        let bins = bin_dir(&["nmcli", "tailscale"]);
        for probe in [
            Probe::TimedOut,
            Probe::Failed("boom".to_string()),
            Probe::Exited {
                code: Some(1),
                output: "acme:vpn\n".to_string(),
            },
            Probe::Exited {
                code: None,
                output: "acme:vpn\n".to_string(),
            },
        ] {
            let runner = FakeRunner::new(&[(NMCLI, probe)]);
            let detected = detect(&sources("linux", &bins, &runner));
            assert_eq!(names(&detected), ["tailscale"]);
        }
    }

    #[test]
    fn wireguard_dir_lists_conf_files() {
        let bins = bin_dir(&["wg-quick"]);
        let runner = FakeRunner::new(&[]);
        let mut src = sources("linux", &bins, &runner);
        let wg = tempfile::tempdir().unwrap();
        for file in ["b.conf", "a.conf", "notes.txt"] {
            std::fs::write(wg.path().join(file), "").unwrap();
        }
        src.wireguard_dir = wg.path().to_path_buf();
        let detected = detect(&src);
        assert_eq!(names(&detected), ["a", "b"]);
        assert_eq!(detected[0].up, "sudo wg-quick up 'a'");
        assert_eq!(detected[0].down, "sudo wg-quick down 'a'");
        assert_eq!(detected[0].status.as_deref(), Some("ip link show 'a'"));
        assert!(!detected[0].template);
    }

    #[test]
    fn unreadable_wireguard_dir_gives_template() {
        let bins = bin_dir(&["wg-quick"]);
        let runner = FakeRunner::new(&[]);
        let src = sources("linux", &bins, &runner);
        let detected = detect(&src);
        assert_eq!(names(&detected), ["wg-quick"]);
        assert!(detected[0].template);
        assert!(detected[0].note.as_deref().unwrap().contains("wireguard"));
        assert!(detected[0].source.contains("wg-quick"));
    }

    #[test]
    fn openvpn_uses_systemd_unit_and_code_3() {
        let bins = bin_dir(&["openvpn"]);
        let runner = FakeRunner::new(&[]);
        let mut src = sources("linux", &bins, &runner);
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("office.conf"), "").unwrap();
        src.openvpn_dir = dir.path().to_path_buf();
        let detected = detect(&src);
        assert_eq!(names(&detected), ["office"]);
        assert_eq!(
            detected[0].up,
            "sudo systemctl start 'openvpn-client@office'"
        );
        assert_eq!(
            detected[0].down,
            "sudo systemctl stop 'openvpn-client@office'"
        );
        assert_eq!(
            detected[0].status.as_deref(),
            Some("systemctl is-active --quiet 'openvpn-client@office'")
        );
        assert_eq!(detected[0].down_codes, Some(vec![3]));

        // An empty directory gives the template.
        let empty = tempfile::tempdir().unwrap();
        src.openvpn_dir = empty.path().to_path_buf();
        let detected = detect(&src);
        assert_eq!(names(&detected), ["openvpn"]);
        assert!(detected[0].template);
        assert_eq!(detected[0].down_codes, Some(vec![3]));
    }

    #[test]
    fn macos_detectors_only_on_macos() {
        let bins = bin_dir(&["scutil", "nmcli"]);
        let (nm_cmd, nm_probe) = FakeRunner::ok(NMCLI, "acme:vpn\n");
        let (sc_cmd, sc_probe) = FakeRunner::ok(
            SCUTIL,
            "Available network connection services in the current set (*=enabled):\n\
             * (Disconnected)   1A2B-3C4D PPP --> L2TP  \"Office VPN\"  [PPP/L2TP]\n\
             (Disconnected)   5E6F-7A8B PPP --> L2TP  \"Disabled\"  [PPP/L2TP]\n",
        );
        let runner = FakeRunner::new(&[(&nm_cmd, nm_probe), (&sc_cmd, sc_probe)]);

        let mac = detect(&sources("macos", &bins, &runner));
        assert_eq!(names(&mac), ["office-vpn"]);
        assert_eq!(mac[0].up, "scutil --nc start 'Office VPN'");
        assert_eq!(
            mac[0].status.as_deref(),
            Some("scutil --nc status 'Office VPN'")
        );
        assert_eq!(mac[0].up_pattern.as_deref(), Some("^Connected"));

        let linux = detect(&sources("linux", &bins, &runner));
        assert_eq!(names(&linux), ["acme"]);
    }

    #[test]
    fn tailscale_on_macos_falls_back_to_the_app_binary() {
        let bins = bin_dir(&[]);
        let runner = FakeRunner::new(&[]);
        let mut src = sources("macos", &bins, &runner);
        let app_dir = bin_dir(&["Tailscale"]);
        src.tailscale_app = app_dir.path().join("Tailscale");
        let detected = detect(&src);
        assert_eq!(names(&detected), ["tailscale"]);
        assert!(!detected[0].verified);
        assert!(detected[0].up.ends_with("/Tailscale' up"));
        assert!(detected[0].up.starts_with('\''));

        // Not available on Linux.
        let mut linux = sources("linux", &bins, &runner);
        linux.tailscale_app = src.tailscale_app.clone();
        assert!(detect(&linux).is_empty());
    }

    #[test]
    fn macos_vendor_presets_are_unverified() {
        let bins = bin_dir(&[
            "tailscale",
            "netbird",
            "openconnect",
            "openfortivpn",
            "mullvad",
            "warp-cli",
        ]);
        let runner = FakeRunner::new(&[]);
        let mac = detect(&sources("macos", &bins, &runner));
        assert_eq!(
            names(&mac),
            [
                "tailscale",
                "netbird",
                "openconnect",
                "openfortivpn",
                "mullvad",
                "warp"
            ]
        );
        assert!(mac.iter().all(|p| !p.verified));
        assert_eq!(mac.iter().filter(|p| p.template).count(), 2);
        let rendered: String = mac.iter().map(render).collect();
        assert_eq!(Config::from_toml(&rendered).unwrap().vpns.len(), 4);
    }

    #[test]
    #[ignore = "reads this machine's real VPN tooling"]
    fn smoke_detect_on_this_machine() {
        let detected = detect(&Sources::system(&ShellRunner));
        for p in &detected {
            println!(
                "{} [{}]{}",
                p.name,
                p.source,
                if p.template { " (template)" } else { "" }
            );
        }
    }

    #[test]
    fn empty_path_entry_is_ignored() {
        let bins = bin_dir(&[]);
        let cwd_bin = std::env::current_dir().unwrap().join("netbird");
        assert!(!cwd_bin.exists());
        let runner = FakeRunner::new(&[]);
        let mut src = sources("linux", &bins, &runner);
        src.path = std::env::join_paths(["", "relative-dir-that-does-not-exist"]).unwrap();
        assert!(!src.has_bin("netbird"));
        assert!(src.path.to_string_lossy().starts_with(':'));
    }

    #[test]
    fn up_patterns_match_realistic_output_through_the_real_config_path() {
        use crate::state::{VpnState, observe};
        let bins = bin_dir(&["protonvpn", "netbird", "nmcli", "mullvad", "scutil"]);
        let (nm_cmd, nm_probe) = FakeRunner::ok(NMCLI, "acme:vpn\n");
        let (sc_cmd, sc_probe) = FakeRunner::ok(SCUTIL, "* (Connected) 1A \"Office\" [PPP/L2TP]\n");
        let runner = FakeRunner::new(&[(&nm_cmd, nm_probe), (&sc_cmd, sc_probe)]);
        let mut presets = detect(&sources("linux", &bins, &runner));
        presets.extend(detect(&sources("macos", &bins, &runner)));

        let up_samples: &[(&str, &str)] = &[
            ("proton", "Status: Connected\nServer: x\n"),
            (
                "netbird",
                "Daemon version: 0.1\nManagement: Connected\nSignal: Connected\n",
            ),
            ("acme", "activated\n"),
            (
                "mullvad",
                "Connected to se-got-wg-001 in Gothenburg, Sweden\n",
            ),
            ("office", "Connected\nExtended Status <dictionary> {\n}\n"),
        ];
        let down_samples: &[(&str, &str)] = &[
            ("proton", "Status: Disconnected\n"),
            ("netbird", "Management: Disconnected\n"),
            ("acme", "\n"),
            ("acme", ""),
            ("mullvad", "Disconnected\n"),
            ("office", "Disconnected\n"),
        ];
        let state = |name: &str, output: &str| {
            let preset = presets.iter().find(|p| p.name == name).unwrap();
            let config = Config::from_toml(&render(preset)).unwrap();
            let probe = Probe::Exited {
                code: Some(0),
                output: output.to_string(),
            };
            observe(&config.vpns[0], &probe).state
        };
        for (name, output) in up_samples {
            assert_eq!(state(name, output), VpnState::Up, "{name}: {output:?}");
        }
        for (name, output) in down_samples {
            assert_eq!(state(name, output), VpnState::Down, "{name}: {output:?}");
        }
    }
}
