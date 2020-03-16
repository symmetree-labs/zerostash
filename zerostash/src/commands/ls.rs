//! `ls` subcommand

use crate::application::app_reader;
use abscissa_core::{Command, Options, Runnable};
use anyhow::{format_err, Error};
use std::process;

/// `ls` subcommand
///
/// The `Options` proc macro generates an option parser based on the struct
/// definition, and is defined in the `gumdrop` crate. See their documentation
/// for a more comprehensive example:
///
/// <https://docs.rs/gumdrop/>
#[derive(Command, Debug, Options)]
pub struct Ls {
    #[options(free)]
    stash: String,

    #[options(free)]
    paths: Vec<String>,
}

impl Runnable for Ls {
    /// Start the application.
    fn run(&self) {
        let app = &*app_reader();
        let mut stash = app.stash_exists(&self.stash);

        for file in stash.list(&self.paths) {
            println!("{}", file.name);
        }
    }
}
