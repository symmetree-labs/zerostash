//! `checkout` subcommand

use crate::prelude::*;

/// `checkout` subcommand
///
/// The `Clap` proc macro generates an option parser based on the struct
/// definition, and is defined in the `gumdrop` crate. See their documentation
/// for a more comprehensive example:
///
/// <https://docs.rs/gumdrop/>
#[derive(Command, Debug, Clap)]
pub struct Checkout {
    stash: String,
    target: String,
    paths: Vec<String>,
}

impl Runnable for Checkout {
    /// Start the application.
    fn run(&self) {
        abscissa_tokio::run(&APP, async {
            let mut stash = APP.stash_exists(&self.stash).await;

            stash
                .restore_by_glob(APP.get_worker_threads(), &self.paths, &self.target)
                .await
                .expect("Error extracting data");
        })
        .unwrap();
    }
}
