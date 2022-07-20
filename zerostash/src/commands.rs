//! Zerostash Subcommands

mod keys;
use keys::*;
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

use crate::{
    config::{Key, SymmetricKey, YubikeyCRConfig, YubikeyCRKey},
    prelude::*,
};
use abscissa_core::{Command, Configurable, Runnable};
use clap::{ArgGroup, Parser};
use std::path::PathBuf;

/// Zerostash Configuration Filename
pub const CONFIG_FILE: &str = "zerostash.toml";

/// Zerostash Subcommands
/// Subcommands need to be listed in an enum.
#[derive(Debug, Parser)]
pub enum ZerostashCmd {
    /// Check out files
    Checkout(Checkout),

    /// Add files to a stash
    Commit(Commit),

    /// List commits in the stash
    Log(Log),

    /// List files in a stash
    Ls(Ls),

    /// Key management & generation
    Keys(Keys),

    /// Delete all data of a stash
    Wipe(Wipe),
}

/// Secure and speedy backups.
///
/// Command line arguments take precedence over the configuration file!
#[derive(Command, Debug, Parser)]
#[clap(author, about, version)]
pub struct EntryPoint {
    #[clap(subcommand)]
    cmd: Box<ZerostashCmd>,

    /// Enable verbose logging
    #[clap(short, long, parse(from_occurrences))]
    pub verbose: usize,

    /// Use config file. Command line args will take precedence!
    #[clap(short, long, value_name = "PATH")]
    pub config: Option<String>,

    /// Use the specified config file
    #[clap(long)]
    pub insecure_config: bool,
}

#[derive(clap::Args, Clone, Debug, Default)]
#[clap(group(
            ArgGroup::new("key")
                .args(&["keyfile", "keystring", "yubikey"]),
        ))]
pub struct StashArgs {
    /// Stash path or alias
    pub stash: String,

    /// Username & password
    #[clap(flatten)]
    pub symmetric_key: SymmetricKey,

    /// Use a keyfile for the stash
    #[clap(short, long, value_name = "PATH")]
    pub keyfile: Option<PathBuf>,

    /// Use a key specification TOML. Eg: '{ source = "yubikey" }'
    #[clap(short = 'K', value_name = "TOML", long)]
    pub keystring: Option<String>,

    /// Use a Yubikey for 2nd factor
    #[clap(short, long)]
    pub yubikey: bool,
}

impl StashArgs {
    pub(crate) fn key(&self) -> Option<Key> {
        let args = self.clone();

        if let Some(path) = args.keyfile {
            Some(Key::KeyFile { path })
        } else if let Some(s) = args.keystring {
            Some(toml::from_str(&s).expect("Invalid TOML"))
        } else if args.yubikey {
            Some(Key::Yubikey(YubikeyCRKey {
                credentials: self.symmetric_key.clone(),
                config: YubikeyCRConfig::default(),
            }))
        } else if !self.symmetric_key.is_empty() {
            Some(Key::Plaintext(self.symmetric_key.clone()))
        } else {
            None
        }
    }

    pub(crate) fn open_with(&self, key: Option<Key>) -> Stash {
        let stash = APP.config().open(&self.stash, key);
        stash.unwrap_or_else(|e| fatal_error(e))
    }

    #[allow(clippy::redundant_closure)]
    pub(crate) fn open(&self) -> Stash {
        self.open_with(self.key())
    }
}

impl Runnable for EntryPoint {
    fn run(&self) {
        use ZerostashCmd::*;
        abscissa_tokio::run(&APP, async move {
            match &*self.cmd {
                Checkout(cmd) => cmd.run().await,
                Commit(cmd) => cmd.run().await,
                Log(cmd) => cmd.run().await,
                Ls(cmd) => cmd.run().await,
                Keys(cmd) => cmd.run().await,
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

                if !self.insecure_config && (file_mode & 0o700) != (file_mode & 0o777) {
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
