//! Zerostash Subcommands
//!
//! This is where you specify the subcommands of your application.
//!
//! The default application comes with two subcommands:
//!
//! - `start`: launches the application
//! - `--version`: print application version
//!
//! See the `impl Configurable` below for how to specify the path to the
//! application's configuration file.

mod alias;
mod checkout;
mod commit;
mod ls;
mod wipe;

use self::{alias::Alias, checkout::Checkout, commit::Commit, ls::Ls, wipe::Wipe};
use crate::config::ZerostashConfig;
use abscissa_core::{Command, Configurable, FrameworkError, Runnable};
use clap::Parser;
use std::path::PathBuf;

/// Zerostash Configuration Filename
pub const CONFIG_FILE: &str = "zerostash.toml";

/// Zerostash Subcommands
/// Subcommands need to be listed in an enum.
#[derive(Command, Debug, Parser, Runnable)]
pub enum ZerostashCmd {
    /// add new alias for a stash URI/path
    Alias(Alias),

    /// check out files
    Checkout(Checkout),

    /// add files to a stash
    Commit(Commit),

    /// list files in a stash
    Ls(Ls),

    /// delete all data of a stash
    Wipe(Wipe),
}

/// Entry point for the application. It needs to be a struct to allow using subcommands!
#[derive(Command, Debug, Parser)]
#[clap(author, about, version)]
pub struct EntryPoint {
    #[clap(subcommand)]
    cmd: ZerostashCmd,

    /// Enable verbose logging
    #[clap(short, long)]
    pub verbose: bool,

    /// Use the specified config file
    #[clap(short, long)]
    pub config: Option<String>,
}

impl Runnable for EntryPoint {
    fn run(&self) {
        self.cmd.run()
    }
}

/// This trait allows you to define how application configuration is loaded.
impl Configurable<ZerostashConfig> for EntryPoint {
    /// Location of the configuration file
    fn config_path(&self) -> Option<PathBuf> {
        let filename = ZerostashConfig::path();

        if filename.exists() {
            Some(filename)
        } else {
            None
        }
    }

    /// Apply changes to the config after it's been loaded, e.g. overriding
    /// values in a config file using command-line options.
    ///
    /// This can be safely deleted if you don't want to override config
    /// settings from command-line options.
    fn process_config(&self, config: ZerostashConfig) -> Result<ZerostashConfig, FrameworkError> {
        Ok(config)
    }
}
