//! Terminal rendering: status table, spinner, interactive picker.

use std::fmt;
use std::io::IsTerminal;
use std::time::Duration;

use anyhow::anyhow;
use indicatif::{ProgressBar, ProgressStyle};
use inquire::ui::{Color, RenderConfig, StyleSheet, Styled};
use inquire::{InquireError, Select};
use owo_colors::{OwoColorize, Stream};

use crate::config::Vpn;
use crate::runner::Runner;
use crate::state::{Observation, VpnState, probe_all};
use crate::switch::{Action, Step};

const NOT_A_TTY: &str = "the interactive picker needs a terminal; use `vpns up <name>` instead";

/// What the user chose in the picker.
#[derive(Debug, PartialEq, Eq)]
pub enum PickerOutcome {
    /// Switch to this VPN (`None`: disconnect all).
    Target(Option<usize>),
    /// Esc: inquire already marks the prompt as canceled.
    Cancelled,
    /// Ctrl-C: the menu is left on screen as is.
    Interrupted,
}

/// Maps a picker error: cancellation is not an error.
pub fn picker_outcome(err: InquireError) -> anyhow::Result<PickerOutcome> {
    match err {
        InquireError::OperationCanceled => Ok(PickerOutcome::Cancelled),
        InquireError::OperationInterrupted => Ok(PickerOutcome::Interrupted),
        InquireError::NotTTY => Err(anyhow!(NOT_A_TTY)),
        err => Err(anyhow::Error::new(err).context("interactive picker failed")),
    }
}

/// Fails early when the picker cannot be shown.
pub fn ensure_tty() -> anyhow::Result<()> {
    if std::io::stdin().is_terminal() && std::io::stderr().is_terminal() {
        Ok(())
    } else {
        Err(anyhow!(NOT_A_TTY))
    }
}

/// Probes every VPN behind a spinner (drawn on stderr, only on a terminal).
pub fn probe_with_spinner(vpns: &[Vpn], runner: &dyn Runner) -> Vec<Observation> {
    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::with_template("{spinner:.cyan} {msg}")
            .unwrap_or_else(|_| ProgressStyle::default_spinner()),
    );
    spinner.set_message("Checking VPN status…");
    spinner.enable_steady_tick(Duration::from_millis(80));
    let observations = probe_all(vpns, runner);
    spinner.finish_and_clear();
    observations
}

/// Prints one aligned line per VPN; `verbose` adds why each state was chosen.
pub fn print_status(vpns: &[Vpn], observations: &[Observation], verbose: bool) {
    let width = name_width(vpns);
    let state_width = observations
        .iter()
        .map(|o| state_label(o.state).len())
        .max()
        .unwrap_or_default();
    for (vpn, seen) in vpns.iter().zip(observations) {
        let row = Row::new(&vpn.name, seen.state, width, Stream::Stdout);
        if verbose {
            let pad = " ".repeat(state_width - state_label(seen.state).len());
            let detail = format!("· {}", seen.detail);
            println!(
                "  {row}{pad}  {}",
                detail.if_supports_color(Stream::Stdout, |s| s.dimmed())
            );
        } else {
            println!("  {row}");
        }
    }
}

fn state_label(state: VpnState) -> &'static str {
    match state {
        VpnState::Up => "up",
        VpnState::Down => "down",
        VpnState::Unknown => "unknown",
    }
}

/// Lets the user pick the VPN to switch to.
pub fn pick(vpns: &[Vpn], states: &[VpnState]) -> anyhow::Result<PickerOutcome> {
    let width = name_width(vpns);
    let mut choices: Vec<Choice> = vpns
        .iter()
        .zip(states)
        .enumerate()
        .map(|(i, (vpn, &state))| Choice {
            label: Row::new(&vpn.name, state, width, Stream::Stderr).to_string(),
            answer: vpn.name.clone(),
            target: Some(i),
        })
        .collect();
    choices.push(Choice {
        label: format!(
            "{} {}",
            "⏻".if_supports_color(Stream::Stderr, |s| s.red()),
            "Disconnect all"
        ),
        answer: "disconnect all".to_string(),
        target: None,
    });
    let cursor = states
        .iter()
        .position(|&s| s == VpnState::Up)
        .unwrap_or_default();

    let answer = Select::new("Switch to", choices)
        .with_starting_cursor(cursor)
        .with_help_message("↑↓ move · enter switch · esc cancel")
        .with_render_config(render_config())
        .with_formatter(&|option| format_answer(option.value))
        .without_filtering()
        .with_vim_mode(true)
        .prompt();
    match answer {
        Ok(choice) => Ok(PickerOutcome::Target(choice.target)),
        Err(err) => picker_outcome(err),
    }
}

/// Announces each command before it runs (commands may prompt, e.g. `sudo`).
pub fn report_step(step: Step) {
    let out = Stream::Stdout;
    match step {
        Step::Starting(Action::Down(_) | Action::EnsureDown(_), name) => println!(
            "{} Stopping {}…",
            "↓".if_supports_color(out, |s| s.yellow()),
            name.if_supports_color(out, |s| s.bold())
        ),
        Step::Starting(Action::Up(_), name) => println!(
            "{} Starting {}…",
            "↑".if_supports_color(out, |s| s.cyan()),
            name.if_supports_color(out, |s| s.bold())
        ),
        Step::Ignored(_, name, exit) => println!(
            "{} {name} down failed ({exit}); its state was unknown, going on",
            "!".if_supports_color(out, |s| s.yellow()),
        ),
        Step::Done(action, name) => println!(
            "{} {} is {}",
            "✓".if_supports_color(out, |s| s.green()),
            name,
            action.verb()
        ),
    }
}

/// Prints an error to stderr.
pub fn print_error(err: &anyhow::Error) {
    eprintln!(
        "{} {err:#}",
        "error:".if_supports_color(Stream::Stderr, |s| s.red().bold().to_string())
    );
}

fn name_width(vpns: &[Vpn]) -> usize {
    vpns.iter()
        .map(|v| v.name.chars().count())
        .max()
        .unwrap_or_default()
}

/// Same rule as every other colored output (`NO_COLOR`, `FORCE_COLOR`, terminal
/// support): ask owo-colors.
fn stderr_has_colors() -> bool {
    "x".if_supports_color(Stream::Stderr, |s| s.red())
        .to_string()
        .len()
        > 1
}

fn render_config() -> RenderConfig<'static> {
    let colors = stderr_has_colors();
    let paint = |text, color| {
        let styled = Styled::new(text);
        if colors {
            styled.with_fg(color)
        } else {
            styled
        }
    };
    let config = if colors {
        RenderConfig::default_colored()
            .with_selected_option(Some(StyleSheet::new().with_fg(Color::LightCyan)))
    } else {
        RenderConfig::empty()
    };
    config
        .with_prompt_prefix(paint("◆", Color::LightCyan))
        .with_answered_prompt_prefix(paint("◇", Color::LightGreen))
        .with_highlighted_option_prefix(paint("›", Color::LightCyan))
}

/// One VPN rendered as `● name   up`.
struct Row<'a> {
    name: &'a str,
    state: VpnState,
    width: usize,
    stream: Stream,
}

impl<'a> Row<'a> {
    fn new(name: &'a str, state: VpnState, width: usize, stream: Stream) -> Self {
        Row {
            name,
            state,
            width,
            stream,
        }
    }
}

impl fmt::Display for Row<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = format!("{:<width$}", self.name, width = self.width);
        let s = self.stream;
        match self.state {
            VpnState::Up => write!(
                f,
                "{} {}  {}",
                "●".if_supports_color(s, |t| t.green()),
                name.if_supports_color(s, |t| t.bold()),
                state_label(VpnState::Up).if_supports_color(s, |t| t.green())
            ),
            VpnState::Down => write!(
                f,
                "{} {}  {}",
                "○".if_supports_color(s, |t| t.dimmed()),
                name,
                state_label(VpnState::Down).if_supports_color(s, |t| t.dimmed())
            ),
            VpnState::Unknown => write!(
                f,
                "{} {}  {}",
                "?".if_supports_color(s, |t| t.yellow()),
                name,
                state_label(VpnState::Unknown).if_supports_color(s, |t| t.yellow())
            ),
        }
    }
}

struct Choice {
    label: String,
    answer: String,
    target: Option<usize>,
}

fn format_answer(choice: &Choice) -> String {
    choice.answer.clone()
}

impl fmt::Display for Choice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picker_outcome_maps_cancel() {
        assert_eq!(
            picker_outcome(InquireError::OperationCanceled).unwrap(),
            PickerOutcome::Cancelled
        );
        assert_eq!(
            picker_outcome(InquireError::OperationInterrupted).unwrap(),
            PickerOutcome::Interrupted
        );
        let err = picker_outcome(InquireError::NotTTY).unwrap_err();
        assert!(err.to_string().contains("vpns up"));
    }

    #[test]
    fn answer_shows_only_the_name() {
        let choice = Choice {
            label: "● netbird  up".to_string(),
            answer: "netbird".to_string(),
            target: Some(2),
        };
        assert_eq!(choice.answer, "netbird");
        assert_eq!(format_answer(&choice), "netbird");
    }
}
