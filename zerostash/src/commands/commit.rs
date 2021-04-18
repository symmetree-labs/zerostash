//! `commit` subcommand

use crate::prelude::*;

/// `commit` subcommand
///
/// The `Clap` proc macro generates an option parser based on the struct
/// definition, and is defined in the `gumdrop` crate. See their documentation
/// for a more comprehensive example:
///
/// <https://docs.rs/gumdrop/>
#[derive(Command, Debug, Clap)]
pub struct Commit {
    stash: String,
    paths: Vec<String>,
}

impl Runnable for Commit {
    /// Start the application.
    fn run(&self) {
        abscissa_tokio::run(&APP, async {
            let mut stash = APP.open_stash(&self.stash);

            for path in self.paths.iter() {
                stash
                    .add_recursive(APP.get_worker_threads(), path)
                    .await
                    .expect("Failed to add path");
            }

            stash.commit().await.expect("Failed to write metadata");
        })
        .unwrap();
    }
}
