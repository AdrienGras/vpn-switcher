use std::path::Path;

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

/// Writes `toml` (with `$D` replaced by the temp dir) and returns the dir.
fn setup(toml: &str) -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    let toml = toml.replace("$D", dir.path().to_str().unwrap());
    std::fs::write(dir.path().join("config.toml"), toml).unwrap();
    dir
}

fn vpns(dir: &Path) -> Command {
    let mut cmd = Command::cargo_bin("vpns").unwrap();
    cmd.arg("--config")
        .arg(dir.join("config.toml"))
        .env_remove("VPNS_CONFIG")
        .env("NO_COLOR", "1");
    cmd
}

/// Two file-backed fake VPNs: a VPN is up when `$D/<name>` exists.
const FILE_VPNS: &str = r#"
[[vpn]]
name = "alpha"
up = "touch $D/alpha"
down = "rm -f $D/alpha"
status = "test -f $D/alpha"

[[vpn]]
name = "beta"
up = "touch $D/beta"
down = "rm -f $D/beta"
status = "test -f $D/beta"
"#;

#[test]
fn status_shows_each_vpn() {
    let dir = setup(
        r#"
        [[vpn]]
        name = "alpha"
        up = "true"
        down = "true"
        status = "true"
        [[vpn]]
        name = "beta"
        up = "true"
        down = "true"
        status = "false"
        [[vpn]]
        name = "gamma"
        up = "true"
        down = "true"
        "#,
    );
    vpns(dir.path())
        .arg("status")
        .assert()
        .success()
        .stdout(predicate::str::is_match(r"alpha\s+up").unwrap())
        .stdout(predicate::str::is_match(r"beta\s+down").unwrap())
        .stdout(predicate::str::is_match(r"gamma\s+unknown").unwrap());
}

#[test]
fn up_switches_and_reports() {
    let dir = setup(FILE_VPNS);
    std::fs::write(dir.path().join("alpha"), "").unwrap();
    vpns(dir.path())
        .args(["up", "beta"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Stopping alpha"))
        .stdout(predicate::str::contains("Starting beta"))
        .stdout(predicate::str::is_match(r"beta\s+up").unwrap());
    assert!(!dir.path().join("alpha").exists());
    assert!(dir.path().join("beta").exists());
}

#[test]
fn up_on_active_vpn_does_nothing() {
    let dir = setup(FILE_VPNS);
    std::fs::write(dir.path().join("beta"), "").unwrap();
    vpns(dir.path())
        .args(["up", "beta"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Nothing to do"));
}

#[test]
fn up_failure_exits_nonzero() {
    let dir = setup(
        r#"
        [[vpn]]
        name = "alpha"
        up = "false"
        down = "true"
        status = "false"
        "#,
    );
    vpns(dir.path())
        .args(["up", "alpha"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "`alpha` up command failed (exit code 1)",
        ));
}

#[test]
fn down_stops_one_vpn() {
    let dir = setup(FILE_VPNS);
    std::fs::write(dir.path().join("alpha"), "").unwrap();
    std::fs::write(dir.path().join("beta"), "").unwrap();
    vpns(dir.path()).args(["down", "alpha"]).assert().success();
    assert!(!dir.path().join("alpha").exists());
    assert!(dir.path().join("beta").exists());
}

#[test]
fn off_downs_everything() {
    let dir = setup(FILE_VPNS);
    std::fs::write(dir.path().join("alpha"), "").unwrap();
    std::fs::write(dir.path().join("beta"), "").unwrap();
    vpns(dir.path()).arg("off").assert().success();
    assert!(!dir.path().join("alpha").exists());
    assert!(!dir.path().join("beta").exists());
}

#[test]
fn unknown_name_lists_known() {
    let dir = setup(FILE_VPNS);
    vpns(dir.path())
        .args(["up", "gamma"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("unknown VPN `gamma`"))
        .stderr(predicate::str::contains("alpha, beta"));
}

#[test]
fn missing_config_suggests_init() {
    let dir = tempfile::tempdir().unwrap();
    vpns(dir.path())
        .arg("status")
        .assert()
        .failure()
        .stderr(predicate::str::contains("config.toml"))
        .stderr(predicate::str::contains("vpns init"));
}

#[test]
fn invalid_config_names_the_problem() {
    let dir = setup("[[vpn]]\nname = \"a\"\nup = \"u\"\ndown = \"d\"\nup_pattern = \"(\"\n");
    vpns(dir.path())
        .arg("status")
        .assert()
        .failure()
        .stderr(predicate::str::contains("VPN `a`: invalid up_pattern"));
}

#[test]
fn init_writes_example_and_refuses_overwrite() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nested").join("config.toml");
    let init = || {
        let mut cmd = Command::cargo_bin("vpns").unwrap();
        cmd.arg("--config").arg(&path).arg("init");
        cmd
    };
    init().assert().success();
    let written = std::fs::read_to_string(&path).unwrap();
    assert!(written.contains("name = \"tailscale\""));
    init()
        .assert()
        .failure()
        .stderr(predicate::str::contains("--force"));
    init().arg("--force").assert().success();
}

#[test]
fn config_env_var_is_used() {
    let dir = setup(FILE_VPNS);
    Command::cargo_bin("vpns")
        .unwrap()
        .env("VPNS_CONFIG", dir.path().join("config.toml"))
        .env("NO_COLOR", "1")
        .arg("status")
        .assert()
        .success()
        .stdout(predicate::str::contains("alpha"));
}

#[test]
fn bare_vpns_without_tty_errors() {
    let dir = setup(FILE_VPNS);
    vpns(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("vpns up"));
}

#[test]
fn empty_config_env_falls_back_to_default_path() {
    let dir = tempfile::tempdir().unwrap();
    let config_dir = dir.path().join("vpn-switcher");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(
        config_dir.join("config.toml"),
        "[[vpn]]\nname = \"alpha\"\nup = \"true\"\ndown = \"true\"\n",
    )
    .unwrap();
    Command::cargo_bin("vpns")
        .unwrap()
        .env("VPNS_CONFIG", "")
        .env("XDG_CONFIG_HOME", dir.path())
        .env("NO_COLOR", "1")
        .arg("status")
        .assert()
        .success()
        .stdout(predicate::str::contains("alpha"));
}

#[test]
fn io_error_shows_path_once() {
    let dir = tempfile::tempdir().unwrap();
    let output = Command::cargo_bin("vpns")
        .unwrap()
        .arg("--config")
        .arg(dir.path())
        .arg("status")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    let path = dir.path().to_str().unwrap();
    assert_eq!(stderr.matches(path).count(), 1, "{stderr}");
}

#[test]
fn verbose_status_explains_state() {
    let dir = setup(
        r#"
        [[vpn]]
        name = "gamma"
        up = "true"
        down = "true"
        status = "exit 3"
        "#,
    );
    vpns(dir.path())
        .args(["-v", "status"])
        .assert()
        .success()
        .stdout(predicate::str::is_match(r"gamma\s+unknown.*exit code 3").unwrap());
    vpns(dir.path())
        .arg("status")
        .assert()
        .success()
        .stdout(predicate::str::contains("exit code").not());
}

#[test]
fn failing_down_of_unknown_vpn_does_not_block_switch() {
    let dir = setup(&format!(
        "{FILE_VPNS}\n[[vpn]]\nname = \"gamma\"\nup = \"true\"\ndown = \"false\"\n"
    ));
    vpns(dir.path())
        .args(["up", "beta"])
        .assert()
        .success()
        .stdout(predicate::str::contains("gamma down failed (exit code 1)"));
    assert!(dir.path().join("beta").exists());
}

#[test]
fn ctrl_c_during_up_reports_state() {
    use nix::sys::signal::{Signal, killpg};
    use nix::unistd::Pid;
    use std::os::unix::process::CommandExt;
    use std::time::Duration;

    let dir = setup(
        r#"
        [[vpn]]
        name = "alpha"
        up = "sleep 5"
        down = "true"
        status = "false"
        "#,
    );
    let child = std::process::Command::new(assert_cmd::cargo::cargo_bin("vpns"))
        .arg("--config")
        .arg(dir.path().join("config.toml"))
        .args(["up", "alpha"])
        .env("NO_COLOR", "1")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .process_group(0)
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(800));
    // What the terminal does on Ctrl-C: SIGINT to the whole foreground group.
    killpg(Pid::from_raw(child.id() as i32), Signal::SIGINT).unwrap();
    let output = child.wait_with_output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(output.status.code(), Some(1), "{stdout}{stderr}");
    assert!(
        stderr.contains("`alpha` up command failed (interrupted)"),
        "{stderr}"
    );
    assert!(stdout.contains("alpha  down"), "{stdout}");
}
