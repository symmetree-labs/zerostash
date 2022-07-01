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

use crate::{
    config::{Key, SymmetricKey, YubikeyCRConfig},
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
                .args(&["set-key", "keyfile", "keystring", "yubikey", "macos_keychain"]),
        ))]
pub struct StashArgs {
    /// Stash path or alias
    pub stash: String,

    /// Use a keyfile for the stash
    #[clap(short, long, value_name = "PATH")]
    pub keyfile: Option<PathBuf>,

    /// Use a key specification TOML. Eg: '{ source = "yubikey" }'
    #[clap(short = 'K', value_name = "TOML", long)]
    pub keystring: Option<String>,

    /// Use a Yubikey for 2nd factor
    #[clap(short, long)]
    pub yubikey: bool,

    /// Username for the stash in macOS Keychain
    #[cfg(target_os = "macos")]
    #[clap(short = 'e', long, value_name = "STASH_USER")]
    pub macos_keychain: Option<String>,
}

impl StashArgs {
    #[allow(clippy::redundant_closure)]
    pub(crate) fn open(&self) -> Stash {
        let key = {
            let args = self.clone();

            if let Some(path) = args.keyfile {
                Some(Key::KeyFile { path })
            } else if let Some(s) = args.keystring {
                Some(toml::from_str(&s).expect("Invalid TOML"))
            } else if args.yubikey {
                Some(Key::YubiKey {
                    credentials: SymmetricKey::default(),
                    config: YubikeyCRConfig::default(),
                })
            } else if cfg!(target_os = "macos") {
                args.macos_keychain.map(|user| Key::MacOsKeychain { user })
            } else {
                None
            }
        };

        let stash = APP.config().open(&self.stash, key);
        stash.unwrap_or_else(|e| fatal_error(e))
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
