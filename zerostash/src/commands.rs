//! Zerostash Subcommands
//!
//! This is where you specify the subcommands of your application.
//!
//! The default application comes with two subcommands:
//!
//! - `start`: launches the application
//! - `version`: print application version
//!
//! See the `impl Configurable` below for how to specify the path to the
//! application's configuration file.

mod alias_add;
mod alias_del;
mod alias_list;
mod checkout;
mod commit;
mod ls;
mod version;
mod wipe;

use self::{
    alias_add::AliasAdd, alias_del::AliasDel, alias_list::AliasList, checkout::Checkout,
    commit::Commit, ls::Ls, version::VersionCmd, wipe::Wipe,
};
use crate::config::ZerostashConfig;
use abscissa_core::{
    config::Override, Command, Configurable, FrameworkError, Help, Options, Runnable,
};
use std::path::PathBuf;

/// Zerostash Configuration Filename
pub const CONFIG_FILE: &str = "zerostash.toml";

/// Zerostash Subcommands
#[derive(Command, Debug, Options, Runnable)]
pub enum ZerostashCmd {
    /// The `help` subcommand
    #[options(help = "get usage information")]
    Help(Help<Self>),

    /// The `version` subcommand
    #[options(help = "display version information")]
    Version(VersionCmd),

    /// The `start` subcommand
    #[options(help = "add new alias for a stash URI/path")]
    AliasAdd(AliasAdd),

    /// The `start` subcommand
    #[options(help = "delete a stash alias")]
    AliasDel(AliasDel),

    /// The `start` subcommand
    #[options(help = "list existing stash shorthands")]
    AliasList(AliasList),

    /// The `start` subcommand
    #[options(help = "check out files")]
    Checkout(Checkout),

    /// The `start` subcommand
    #[options(help = "add files to a stash")]
    Commit(Commit),

    /// The `start` subcommand
    #[options(help = "list files in a stash")]
    Ls(Ls),

    /// The `start` subcommand
    #[options(help = "delete all data of a stash")]
    Wipe(Wipe),
}

/// This trait allows you to define how application configuration is loaded.
impl Configurable<ZerostashConfig> for ZerostashCmd {
    /// Location of the configuration file
    fn config_path(&self) -> Option<PathBuf> {
        // Check if the config file exists, and if it does not, ignore it.
        // If you'd like for a missing configuration file to be a hard error
        // instead, always return `Some(CONFIG_FILE)` here.
        let filename = PathBuf::from(CONFIG_FILE);

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
