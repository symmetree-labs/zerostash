//! `ls` subcommand

use crate::prelude::*;
use clap::Parser;

/// `ls` subcommand
///
/// The `Clap` proc macro generates an option parser based on the struct
/// definition, and is defined in the `gumdrop` crate. See their documentation
/// for a more comprehensive example:
///
/// <https://docs.rs/gumdrop/>
#[derive(Command, Debug, Parser)]
pub struct Ls {
    stash: String,

    #[clap(flatten)]
    options: zerostash_files::restore::Options,
}

impl Runnable for Ls {
    /// Start the application.
    fn run(&self) {
        let mut stash = APP.open_stash(&self.stash);

        stash.load_all().unwrap();
        self.options.list(&stash);
    }
}
