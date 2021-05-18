//! `ls` subcommand

use crate::prelude::*;

/// `ls` subcommand
///
/// The `Clap` proc macro generates an option parser based on the struct
/// definition, and is defined in the `gumdrop` crate. See their documentation
/// for a more comprehensive example:
///
/// <https://docs.rs/gumdrop/>
#[derive(Command, Debug, Clap)]
pub struct Ls {
    stash: String,
    paths: Vec<String>,
}

impl Runnable for Ls {
    /// Start the application.
    fn run(&self) {
        let stash = APP.open_stash(&self.stash);

        for file in stash.index().list(&self.paths) {
            println!("{}", file.name);
        }
    }
}
