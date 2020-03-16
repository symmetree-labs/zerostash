//! `commit` subcommand

use crate::application::app_reader;
use abscissa_core::{Command, Options, Runnable};

/// `commit` subcommand
///
/// The `Options` proc macro generates an option parser based on the struct
/// definition, and is defined in the `gumdrop` crate. See their documentation
/// for a more comprehensive example:
///
/// <https://docs.rs/gumdrop/>
#[derive(Command, Debug, Options)]
pub struct Commit {
    #[options(free)]
    stash: String,

    #[options(free)]
    paths: Vec<String>,
}

impl Runnable for Commit {
    /// Start the application.
    fn run(&self) {
        let app = &*app_reader();
        let mut stash = app.open_stash(&self.stash);

        for path in self.paths.iter() {
            stash
                .add_recursive(app.get_worker_threads(), path)
                .expect("Failed to add path");
        }

        stash.commit().expect("Failed to write metadata");
    }
}
