//! Presets produced by VPN detection, and their rendering to `config.toml`.
//!
//! Everything here is pure: no process is spawned and no file is touched.

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
}
