//! Zerostash Subcommands

mod checkout;
use checkout::*;
mod commit;
use commit::*;
mod log;
use log::*;
mod ls;
use ls::*;
mod wipe;
use wipe::*;

use crate::prelude::*;
use abscissa_core::{Command, Configurable, Runnable};
use clap::Parser;
use std::path::PathBuf;

/// Zerostash Configuration Filename
pub const CONFIG_FILE: &str = "zerostash.toml";

/// Zerostash Subcommands
/// Subcommands need to be listed in an enum.
#[derive(Debug, Parser, Clone)]
pub enum ZerostashCmd {
    /// Check out files
    Checkout(Checkout),

    /// Add files to a stash
    Commit(Commit),

    /// List commits in the stash
    Log(Log),

    /// List files in a stash
    Ls(Ls),

    /// Delete all data of a stash
    Wipe(Wipe),
}

/// Secure and speedy backups.
#[derive(Command, Debug, Parser)]
#[clap(author, about, version)]
pub struct EntryPoint {
    #[clap(subcommand)]
    cmd: ZerostashCmd,

    /// Enable verbose logging
    #[clap(short, long, parse(from_occurrences))]
    pub verbose: usize,

    /// Use the specified config file
    #[clap(short, long)]
    pub config: Option<String>,
}

impl Runnable for EntryPoint {
    fn run(&self) {
        use ZerostashCmd::*;
        let command = self.cmd.clone();
        abscissa_tokio::run(&APP, async move {
            match command {
                Checkout(cmd) => cmd.run().await,
                Commit(cmd) => cmd.run().await,
                Log(cmd) => cmd.run().await,
                Ls(cmd) => cmd.run().await,
                Wipe(cmd) => cmd.run().await,
            }
        })
        .unwrap()
    }
}

/// This trait allows you to define how application configuration is loaded.
impl Configurable<ZerostashConfig> for EntryPoint {
    /// Location of the configuration file
    fn config_path(&self) -> Option<PathBuf> {
        let filename = self
            .config
            .as_ref()
            .map(PathBuf::from)
            .unwrap_or_else(ZerostashConfig::path);

        if filename.exists() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                let file_mode = std::fs::metadata(&filename).ok()?.mode();

                if (file_mode & 0o700) != (file_mode & 0o777) {
                    panic!(
                        "Config file {filename:?} must not be accessible for other users! Try running `chmod 600 {filename:?}`"
                    )
                }
            }

            Some(filename)
        } else {
            None
        }
    }
}
