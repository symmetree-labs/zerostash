//! `checkout` subcommand

use crate::prelude::*;
use clap::Parser;
use zerostash_files::restore;

/// `checkout` subcommand
///
/// The `Clap` proc macro generates an option parser based on the struct
/// definition, and is defined in the `gumdrop` crate. See their documentation
/// for a more comprehensive example:
///
/// <https://docs.rs/gumdrop/>
#[derive(Command, Debug, Parser)]
pub struct Checkout {
    stash: String,

    #[clap(flatten)]
    options: restore::Options,
}

impl Runnable for Checkout {
    /// Start the application.
    fn run(&self) {
        abscissa_tokio::run(&APP, async {
            let mut stash = APP.stash_exists(&self.stash).await;
            stash.load_all().unwrap();

            self.options
                .from_iter(&stash, APP.get_worker_threads())
                .await
                .expect("Error extracting data");
        })
        .unwrap();
    }
}
