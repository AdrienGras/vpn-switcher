//! Command-line interface definition.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

/// Switch between mutually incompatible VPNs.
///
/// Run without a command to pick the VPN to switch to with the arrow keys.
#[derive(Debug, Parser)]
#[command(name = "vpns", version, about, long_about)]
pub struct Cli {
    /// Configuration file [default: $VPNS_CONFIG, then $XDG_CONFIG_HOME/vpn-switcher/config.toml]
    #[arg(long, short, global = true, value_name = "PATH")]
    pub config: Option<PathBuf>,

    /// Explain how each VPN state was determined
    #[arg(long, short, global = true)]
    pub verbose: bool,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Pick a VPN interactively and switch to it (default)
    Switch,
    /// Show the state of every VPN
    Status,
    /// Switch to a VPN: stop the others, then start it
    Up {
        /// VPN name, as in the configuration
        name: String,
    },
    /// Stop a VPN
    Down {
        /// VPN name, as in the configuration
        name: String,
    },
    /// Stop every VPN
    Off,
    /// Write an example configuration file
    Init {
        /// Overwrite an existing file
        #[arg(long)]
        force: bool,
    },
}
