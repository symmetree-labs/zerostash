//! `checkout` subcommand

use crate::application::{app_reader, fatal_error};
use abscissa_core::{Command, Options, Runnable};

/// `checkout` subcommand
///
/// The `Options` proc macro generates an option parser based on the struct
/// definition, and is defined in the `gumdrop` crate. See their documentation
/// for a more comprehensive example:
///
/// <https://docs.rs/gumdrop/>
#[derive(Command, Debug, Options)]
pub struct Checkout {
    #[options(free)]
    stash: String,

    #[options(free)]
    target: String,

    #[options(free)]
    paths: Vec<String>,
}

impl Runnable for Checkout {
    /// Start the application.
    fn run(&self) {
        let app = &*app_reader();
        let mut stash = app.stash_exists(&self.stash);

        stash
            .restore_by_glob(app.get_worker_threads(), &self.paths, &self.target)
            .expect("Error extracting data");
    }
}
