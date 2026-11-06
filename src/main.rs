use std::path::Path;
use std::process::ExitCode;
use std::time::Duration;

use anyhow::{Context as _, anyhow, bail};
use clap::Parser;

use vpn_switcher::cli::{Cli, Command, ConfigAction};
use vpn_switcher::config::{self, Config, ConfigError};
use vpn_switcher::interrupt::{self, Disposition};
use vpn_switcher::runner::{self, Runner, ShellRunner};
use vpn_switcher::state::{self, VpnState};
use vpn_switcher::switch::{self, Action, Step, SwitchError};
use vpn_switcher::ui::{self, PickerOutcome};

/// Exit code after Ctrl-C (128 + SIGINT), as shells do.
const INTERRUPTED: i32 = 130;

fn main() -> ExitCode {
    // Ctrl-C, SIGTERM, SIGHUP. During a switch, the first one stops the running
    // command and skips the rest (see `interrupt`). Otherwise kill the status
    // probes, which run in their own process groups out of the terminal's reach.
    let _ = ctrlc::set_handler(|| {
        if interrupt::on_signal() == Disposition::Exit {
            runner::kill_live_probes();
            std::process::exit(INTERRUPTED);
        }
    });
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            ui::print_error(&err);
            match err.downcast_ref::<SwitchError>() {
                Some(SwitchError::Interrupted) => ExitCode::from(INTERRUPTED as u8),
                _ => ExitCode::FAILURE,
            }
        }
    }
}

fn run(cli: Cli) -> anyhow::Result<()> {
    let path = config::resolve_path(cli.config)?;
    let ctx = Context {
        runner: &ShellRunner,
        verbose: cli.verbose,
    };
    match cli.command.unwrap_or(Command::Switch) {
        Command::Init { force } => init(&path, force),
        Command::Config { action } => match action {
            ConfigAction::Path => {
                println!("{}", path.display());
                if !path.exists() {
                    eprintln!(
                        "note: {} does not exist yet; run `vpns init` to create it",
                        path.display()
                    );
                }
                Ok(())
            }
            ConfigAction::Edit => {
                edit(&path)?;
                check(&ctx, &path)
            }
            ConfigAction::Check => check(&ctx, &path),
        },
        Command::Switch => {
            ui::ensure_tty()?;
            let config = load(&path)?;
            let states = ctx.probe(&config);
            match ui::pick(&config.vpns, &states)? {
                PickerOutcome::Target(target) => {
                    ctx.apply(&config, &switch::plan_switch(&states, target))
                }
                // inquire already prints `<canceled>` on the prompt line.
                PickerOutcome::Cancelled => Ok(()),
                // Ctrl-C leaves the menu drawn as is: say that nothing happened.
                PickerOutcome::Interrupted => {
                    println!("\nCancelled.");
                    Ok(())
                }
            }
        }
        Command::Status => {
            let config = load(&path)?;
            ctx.show(&config);
            Ok(())
        }
        Command::Up { name } => {
            let config = load(&path)?;
            let index = find(&config, &name)?;
            let states = ctx.probe(&config);
            ctx.apply(&config, &switch::plan_switch(&states, Some(index)))
        }
        Command::Down { name } => {
            let config = load(&path)?;
            let index = find(&config, &name)?;
            let states = ctx.probe(&config);
            ctx.apply(&config, &switch::plan_down(&states, index))
        }
        Command::Off => {
            let config = load(&path)?;
            let states = ctx.probe(&config);
            ctx.apply(&config, &switch::plan_switch(&states, None))
        }
    }
}

struct Context<'a> {
    runner: &'a dyn Runner,
    verbose: bool,
}

impl Context<'_> {
    /// Current states, for planning.
    fn probe(&self, config: &Config) -> Vec<VpnState> {
        state::states(&ui::probe_with_spinner(&config.vpns, self.runner))
    }

    /// Probes and prints the status table.
    fn show(&self, config: &Config) {
        let observations = ui::probe_with_spinner(&config.vpns, self.runner);
        ui::print_status(&config.vpns, &observations, self.verbose);
    }

    /// Runs the actions, then shows the resulting state even if one of them failed.
    ///
    /// A best-effort stop that failed does not stop the switch, but still makes
    /// the command fail: that VPN may be up.
    fn apply(&self, config: &Config, actions: &[Action]) -> anyhow::Result<()> {
        if actions.is_empty() {
            println!("Nothing to do.");
            return Ok(());
        }
        let mut ignored = Vec::new();
        let result = {
            let _switch = interrupt::begin_switch();
            let result = switch::execute(
                actions,
                &config.vpns,
                self.runner,
                &interrupt::requested,
                |step| {
                    if let Step::Ignored(_, name, exit) = &step {
                        ignored.push(format!(
                            "`{name}` may still be up: its down command failed ({exit})"
                        ));
                    }
                    ui::report_step(step);
                },
            );
            if matches!(result, Err(SwitchError::Interrupted)) {
                interrupt::settle(Duration::from_millis(500));
            }
            result
        };
        println!();
        self.show(config);
        result?;
        match ignored.is_empty() {
            true => Ok(()),
            false => Err(anyhow!(ignored.join("\n"))),
        }
    }
}

fn load(path: &Path) -> anyhow::Result<Config> {
    Config::load(path).map_err(|err| match err {
        ConfigError::NotFound(path) => anyhow!(
            "no configuration file at {}\n  run `vpns init` to create one",
            path.display()
        ),
        // Already names the file.
        err @ ConfigError::Io { .. } => err.into(),
        err => anyhow::Error::new(err).context(format!("in {}", path.display())),
    })
}

/// Validates the configuration and probes every status command.
fn check(ctx: &Context, path: &Path) -> anyhow::Result<()> {
    let config = load(path)?;
    let observations = ui::probe_with_spinner(&config.vpns, ctx.runner);
    ui::print_check(path, &config.vpns, &observations);
    match state::unreadable(&config.vpns, &observations).len() {
        0 => Ok(()),
        1 => bail!("1 VPN cannot be read: its status command is probably wrong"),
        n => bail!("{n} VPNs cannot be read: their status commands are probably wrong"),
    }
}

/// Opens the configuration in `$VISUAL`, `$EDITOR` or `vi`, on the terminal.
fn edit(path: &Path) -> anyhow::Result<()> {
    if !path.exists() {
        load(path)?;
    }
    let editor = ["VISUAL", "EDITOR"]
        .iter()
        .filter_map(|var| std::env::var(var).ok())
        .find(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "vi".to_string());
    // The path is passed as `$1`, so spaces and quotes in it are safe.
    std::process::Command::new("sh")
        .args(["-c", &format!("{editor} \"$1\""), "vpns-edit"])
        .arg(path)
        .status()
        .with_context(|| format!("cannot run editor `{editor}`"))?;
    Ok(())
}

fn find(config: &Config, name: &str) -> anyhow::Result<usize> {
    config
        .find(name)
        .ok_or_else(|| anyhow!("unknown VPN `{name}` (known: {})", config.names()))
}

fn init(path: &Path, force: bool) -> anyhow::Result<()> {
    if path.exists() && !force {
        bail!(
            "{} already exists (use --force to overwrite)",
            path.display()
        );
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("cannot create {}", parent.display()))?;
    }
    std::fs::write(path, config::EXAMPLE)
        .with_context(|| format!("cannot write {}", path.display()))?;
    println!("Wrote an example configuration to {}", path.display());
    println!("Edit it, then run `vpns`.");
    Ok(())
}
