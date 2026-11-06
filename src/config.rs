//! Configuration loading and validation.

use std::path::{Path, PathBuf};

use etcetera::BaseStrategy;
use regex::Regex;
use serde::Deserialize;

/// Environment variable overriding the configuration path.
pub const CONFIG_ENV: &str = "VPNS_CONFIG";

/// Example configuration written by `vpns init`.
pub const EXAMPLE: &str = r#"# vpn-switcher configuration
#
# Each [[vpn]] table describes one VPN:
#   name        identifier used on the command line
#   up / down   shell commands that start / stop the VPN
#   status      optional shell command reporting the state
#   up_pattern  optional regex: the VPN is up when status output matches it
#   up_codes    status exit codes meaning "up"   (default [0])
#   down_codes  status exit codes meaning "down" (default [1])

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
"#;

/// A validated VPN definition.
#[derive(Debug, Clone)]
pub struct Vpn {
    pub name: String,
    pub up: String,
    pub down: String,
    pub status: Option<String>,
    pub up_pattern: Option<Regex>,
    pub up_codes: Vec<i32>,
    pub down_codes: Vec<i32>,
}

/// A validated configuration.
#[derive(Debug, Clone)]
pub struct Config {
    pub vpns: Vec<Vpn>,
}

#[derive(thiserror::Error, Debug)]
pub enum ConfigError {
    #[error("no configuration file at {}", .0.display())]
    NotFound(PathBuf),
    #[error("cannot read {}", path.display())]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("invalid configuration")]
    Parse(#[from] toml::de::Error),
    #[error("the configuration defines no VPN (add a [[vpn]] table)")]
    Empty,
    #[error("a VPN has an empty name")]
    BlankName,
    #[error("VPN `{0}` is defined more than once")]
    DuplicateName(String),
    #[error("VPN `{name}`: invalid up_pattern")]
    InvalidPattern { name: String, source: regex::Error },
    #[error("cannot determine the user configuration directory")]
    NoConfigDir,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    #[serde(rename = "vpn", default)]
    vpn: Vec<RawVpn>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawVpn {
    name: String,
    up: String,
    down: String,
    status: Option<String>,
    up_pattern: Option<String>,
    #[serde(default = "default_up_codes")]
    up_codes: Vec<i32>,
    #[serde(default = "default_down_codes")]
    down_codes: Vec<i32>,
}

fn default_up_codes() -> Vec<i32> {
    vec![0]
}

fn default_down_codes() -> Vec<i32> {
    vec![1]
}

impl TryFrom<RawVpn> for Vpn {
    type Error = ConfigError;

    fn try_from(raw: RawVpn) -> Result<Self, Self::Error> {
        let up_pattern = raw
            .up_pattern
            .as_deref()
            .map(Regex::new)
            .transpose()
            .map_err(|source| ConfigError::InvalidPattern {
                name: raw.name.clone(),
                source,
            })?;
        Ok(Vpn {
            name: raw.name,
            up: raw.up,
            down: raw.down,
            status: raw.status,
            up_pattern,
            up_codes: raw.up_codes,
            down_codes: raw.down_codes,
        })
    }
}

impl Config {
    /// Parses and validates a TOML configuration.
    pub fn from_toml(s: &str) -> Result<Config, ConfigError> {
        let raw: RawConfig = toml::from_str(s)?;
        if raw.vpn.is_empty() {
            return Err(ConfigError::Empty);
        }
        let mut vpns: Vec<Vpn> = Vec::with_capacity(raw.vpn.len());
        for raw_vpn in raw.vpn {
            if raw_vpn.name.trim().is_empty() {
                return Err(ConfigError::BlankName);
            }
            if vpns.iter().any(|v| v.name == raw_vpn.name) {
                return Err(ConfigError::DuplicateName(raw_vpn.name));
            }
            vpns.push(raw_vpn.try_into()?);
        }
        Ok(Config { vpns })
    }

    /// Reads and validates the configuration file at `path`.
    pub fn load(path: &Path) -> Result<Config, ConfigError> {
        let content = std::fs::read_to_string(path).map_err(|source| {
            if source.kind() == std::io::ErrorKind::NotFound {
                ConfigError::NotFound(path.to_path_buf())
            } else {
                ConfigError::Io {
                    path: path.to_path_buf(),
                    source,
                }
            }
        })?;
        Config::from_toml(&content)
    }

    /// Index of the VPN called `name`.
    pub fn find(&self, name: &str) -> Option<usize> {
        self.vpns.iter().position(|v| v.name == name)
    }

    /// Comma-separated list of the configured VPN names.
    pub fn names(&self) -> String {
        self.vpns
            .iter()
            .map(|v| v.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// The leading comment lines of [`EXAMPLE`], each ending with a newline.
pub fn example_header() -> String {
    EXAMPLE
        .lines()
        .take_while(|line| line.starts_with('#'))
        .map(|line| format!("{line}\n"))
        .collect()
}

/// Names of the VPNs defined in `existing`, which must be a valid configuration.
pub fn names_in(existing: &str) -> Result<Vec<String>, ConfigError> {
    Ok(Config::from_toml(existing)?
        .vpns
        .into_iter()
        .map(|v| v.name)
        .collect())
}

/// `existing` followed by `blocks`, keeping every byte of `existing` as is.
///
/// Refuses an invalid `existing`: appending to it would hide its error.
pub fn append_presets(existing: &str, blocks: &[String]) -> Result<String, ConfigError> {
    Config::from_toml(existing)?;
    let mut out = existing.to_string();
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out.push('\n');
    out.push_str(&blocks.join("\n"));
    Ok(out)
}

/// `$XDG_CONFIG_HOME/vpn-switcher/config.toml` (or `~/.config/...`).
pub fn default_path() -> Result<PathBuf, ConfigError> {
    let base = etcetera::choose_base_strategy().map_err(|_| ConfigError::NoConfigDir)?;
    Ok(base.config_dir().join("vpn-switcher").join("config.toml"))
}

/// Configuration path: `--config` flag, then `VPNS_CONFIG`, then the default.
pub fn resolve_path(flag: Option<PathBuf>) -> Result<PathBuf, ConfigError> {
    if let Some(path) = flag {
        return Ok(path);
    }
    match std::env::var_os(CONFIG_ENV) {
        Some(path) if !path.is_empty() => Ok(PathBuf::from(path)),
        _ => default_path(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL: &str = r#"
        [[vpn]]
        name = "a"
        up = "u"
        down = "d"
    "#;

    #[test]
    fn parses_minimal_vpn_with_default_codes() {
        let config = Config::from_toml(MINIMAL).unwrap();
        let vpn = &config.vpns[0];
        assert_eq!(vpn.name, "a");
        assert_eq!(vpn.up, "u");
        assert_eq!(vpn.down, "d");
        assert_eq!(vpn.status, None);
        assert!(vpn.up_pattern.is_none());
        assert_eq!(vpn.up_codes, vec![0]);
        assert_eq!(vpn.down_codes, vec![1]);
    }

    #[test]
    fn parses_pattern_and_custom_codes() {
        let config = Config::from_toml(
            r#"
            [[vpn]]
            name = "a"
            up = "u"
            down = "d"
            status = "s"
            up_pattern = "Status: Connected"
            up_codes = [0, 2]
            "#,
        )
        .unwrap();
        let vpn = &config.vpns[0];
        assert_eq!(vpn.status.as_deref(), Some("s"));
        assert!(
            vpn.up_pattern
                .as_ref()
                .unwrap()
                .is_match("Status: Connected")
        );
        assert_eq!(vpn.up_codes, vec![0, 2]);
        assert_eq!(vpn.down_codes, vec![1]);
    }

    #[test]
    fn rejects_invalid_pattern() {
        let err = Config::from_toml(
            r#"
            [[vpn]]
            name = "a"
            up = "u"
            down = "d"
            up_pattern = "("
            "#,
        )
        .unwrap_err();
        assert!(matches!(err, ConfigError::InvalidPattern { ref name, .. } if name == "a"));
    }

    #[test]
    fn rejects_duplicate_names() {
        let toml = format!("{MINIMAL}{MINIMAL}");
        let err = Config::from_toml(&toml).unwrap_err();
        assert!(matches!(err, ConfigError::DuplicateName(ref n) if n == "a"));
    }

    #[test]
    fn rejects_blank_name() {
        let toml = MINIMAL.replace("\"a\"", "\"  \"");
        assert!(matches!(
            Config::from_toml(&toml).unwrap_err(),
            ConfigError::BlankName
        ));
    }

    #[test]
    fn rejects_empty_config() {
        assert!(matches!(
            Config::from_toml("").unwrap_err(),
            ConfigError::Empty
        ));
    }

    #[test]
    fn rejects_unknown_key() {
        let toml = format!("{MINIMAL}\nfoo = 1\n");
        assert!(matches!(
            Config::from_toml(&toml).unwrap_err(),
            ConfigError::Parse(_)
        ));
    }

    #[test]
    fn example_config_is_valid() {
        let config = Config::from_toml(EXAMPLE).unwrap();
        let names: Vec<_> = config.vpns.iter().map(|v| v.name.as_str()).collect();
        assert_eq!(names, ["proton", "tailscale", "netbird"]);
    }

    #[test]
    fn find_returns_index() {
        let toml = format!("{MINIMAL}{}", MINIMAL.replace("\"a\"", "\"b\""));
        let config = Config::from_toml(&toml).unwrap();
        assert_eq!(config.find("b"), Some(1));
        assert_eq!(config.find("zzz"), None);
    }

    #[test]
    fn load_missing_file_is_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        assert!(matches!(
            Config::load(&path).unwrap_err(),
            ConfigError::NotFound(p) if p == path
        ));
    }

    #[test]
    fn flag_path_wins() {
        let path = PathBuf::from("/tmp/explicit.toml");
        assert_eq!(resolve_path(Some(path.clone())).unwrap(), path);
    }

    const HAND_WRITTEN: &str = "# mine\n[[vpn]]\nname = \"a\"\nup = \"u\"\ndown = \"d\"";

    #[test]
    fn append_preserves_existing_bytes() {
        let block = "[[vpn]]\nname = \"b\"\nup = \"u\"\ndown = \"d\"\n".to_string();
        let out = append_presets(HAND_WRITTEN, &[block]).unwrap();
        assert!(out.starts_with(HAND_WRITTEN));
        assert_eq!(Config::from_toml(&out).unwrap().vpns.len(), 2);
        // With a trailing newline: one blank line separates the blocks.
        let with_newline = format!("{HAND_WRITTEN}\n");
        let out = append_presets(&with_newline, &["# x\n".to_string()]).unwrap();
        assert!(out.starts_with(&with_newline));
        assert!(out.ends_with("\n\n# x\n"));
    }

    #[test]
    fn append_refuses_invalid_existing() {
        let dup = format!("{HAND_WRITTEN}\n{HAND_WRITTEN}\n");
        assert!(matches!(
            append_presets(&dup, &[]),
            Err(ConfigError::DuplicateName(_))
        ));
    }

    #[test]
    fn names_in_lists_names() {
        assert_eq!(names_in(HAND_WRITTEN).unwrap(), ["a"]);
        assert!(names_in("[[vpn]]").is_err());
    }

    #[test]
    fn example_header_is_the_leading_comments() {
        let header = example_header();
        assert!(header.starts_with("# vpn-switcher configuration"));
        assert!(header.lines().all(|l| l.starts_with('#')));
        assert!(header.ends_with('\n'));
        assert!(matches!(
            Config::from_toml(&header),
            Err(ConfigError::Empty)
        ));
    }
}
