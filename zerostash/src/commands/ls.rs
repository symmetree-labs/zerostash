//! `ls` subcommand

use crate::application::APP;
use abscissa_core::{Clap, Command, Runnable};
use anyhow::{format_err, Error};
use std::process;

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
        let mut stash = APP.stash_exists(&self.stash);

        for file in stash.list(&self.paths) {
            println!("{}", file.name);
        }
    }
}
