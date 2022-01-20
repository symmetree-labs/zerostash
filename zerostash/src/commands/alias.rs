//! `alias-add` subcommand

use crate::prelude::*;
use clap::Parser;

/// `alias-add` subcommand
///
/// The `Options` proc macro generates an option parser based on the struct
/// definition, and is defined in the `gumdrop` crate. See their documentation
/// for a more comprehensive example:
///
/// <https://docs.rs/gumdrop/>
#[derive(Command, Debug, Parser)]
pub struct Alias {}

impl Runnable for Alias {
    /// Start the application.
    fn run(&self) {
        // Your code goes here
    }
}
