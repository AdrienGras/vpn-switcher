use std::path::Path;
use std::process::ExitCode;

use anyhow::{Context as _, anyhow, bail};
use clap::Parser;

use vpn_switcher::cli::{Cli, Command};
use vpn_switcher::config::{self, Config, ConfigError};
use vpn_switcher::runner::{self, Runner, ShellRunner};
use vpn_switcher::state::{self, VpnState};
use vpn_switcher::switch::{self, Action};
use vpn_switcher::ui::{self, PickerOutcome};

fn main() -> ExitCode {
    // Ctrl-C: an interactive up/down command gets the terminal's SIGINT itself, and
    // `vpns` stays alive to report how it ended. Otherwise kill the status probes,
    // which run in their own process groups out of the terminal's reach, and leave.
    let _ = ctrlc::set_handler(|| {
        if !runner::interactive_child_running() {
            runner::kill_live_probes();
            std::process::exit(130);
        }
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
    let ctx = Context {
        runner: &ShellRunner,
        verbose: cli.verbose,
    };
    match cli.command.unwrap_or(Command::Switch) {
        Command::Init { force } => init(&path, force),
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
    fn apply(&self, config: &Config, actions: &[Action]) -> anyhow::Result<()> {
        if actions.is_empty() {
            println!("Nothing to do.");
            return Ok(());
        }
        let result = switch::execute(actions, &config.vpns, self.runner, ui::report_step);
        println!();
        self.show(config);
        Ok(result?)
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
