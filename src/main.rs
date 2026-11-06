use std::path::Path;
use std::process::ExitCode;

use anyhow::{Context, anyhow, bail};
use clap::Parser;

use vpn_switcher::cli::{Cli, Command};
use vpn_switcher::config::{self, Config, ConfigError};
use vpn_switcher::runner::{self, Runner, ShellRunner};
use vpn_switcher::switch::{self, Action};
use vpn_switcher::ui::{self, PickerOutcome};

fn main() -> ExitCode {
    // Status probes run in their own process groups, out of reach of the
    // terminal's SIGINT: take them down with us.
    let _ = ctrlc::set_handler(|| {
        runner::kill_live_probes();
        std::process::exit(130);
    });
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            ui::print_error(&err);
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> anyhow::Result<()> {
    let path = config::resolve_path(cli.config)?;
    let runner = ShellRunner;
    match cli.command.unwrap_or(Command::Switch) {
        Command::Init { force } => init(&path, force),
        Command::Switch => {
            ui::ensure_tty()?;
            let config = load(&path)?;
            let states = ui::probe_with_spinner(&config.vpns, &runner);
            match ui::pick(&config.vpns, &states)? {
                PickerOutcome::Target(target) => {
                    apply(&config, &runner, &switch::plan_switch(&states, target))
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
            let states = ui::probe_with_spinner(&config.vpns, &runner);
            ui::print_status(&config.vpns, &states);
            Ok(())
        }
        Command::Up { name } => {
            let config = load(&path)?;
            let index = find(&config, &name)?;
            let states = ui::probe_with_spinner(&config.vpns, &runner);
            apply(&config, &runner, &switch::plan_switch(&states, Some(index)))
        }
        Command::Down { name } => {
            let config = load(&path)?;
            let index = find(&config, &name)?;
            let states = ui::probe_with_spinner(&config.vpns, &runner);
            apply(&config, &runner, &switch::plan_down(&states, index))
        }
        Command::Off => {
            let config = load(&path)?;
            let states = ui::probe_with_spinner(&config.vpns, &runner);
            apply(&config, &runner, &switch::plan_switch(&states, None))
        }
    }
}

/// Runs the actions, then shows the resulting state even if one of them failed.
fn apply(config: &Config, runner: &dyn Runner, actions: &[Action]) -> anyhow::Result<()> {
    if actions.is_empty() {
        println!("Nothing to do.");
        return Ok(());
    }
    let result = switch::execute(actions, &config.vpns, runner, ui::report_step);
    println!();
    let states = ui::probe_with_spinner(&config.vpns, runner);
    ui::print_status(&config.vpns, &states);
    Ok(result?)
}

fn load(path: &Path) -> anyhow::Result<Config> {
    Config::load(path).map_err(|err| match err {
        ConfigError::NotFound(path) => anyhow!(
            "no configuration file at {}\n  run `vpns init` to create one",
            path.display()
        ),
        err => anyhow::Error::new(err).context(format!("in {}", path.display())),
    })
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
