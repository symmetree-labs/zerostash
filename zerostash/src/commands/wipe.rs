//! `wipe` subcommand

use crate::prelude::*;

/// `wipe` subcommand
///
/// The `Clap` proc macro generates an option parser based on the struct
/// definition, and is defined in the `gumdrop` crate. See their documentation
/// for a more comprehensive example:
///
/// <https://docs.rs/gumdrop/>
#[derive(Command, Debug, Clap)]
pub struct Wipe {
    stash: String,
}

impl Runnable for Wipe {
    /// Start the application.
    fn run(&self) {
        use crate::config::Backend::*;

        let config = APP.config.read();
        let path = match config.resolve_stash(&self.stash) {
            None => self.stash.clone(),
            Some(stash) => match &stash.backend {
                Filesystem { path } => path.clone(),
            },
        };

        std::fs::remove_dir_all(path).expect("Error while wiping stash...");
    }
}
