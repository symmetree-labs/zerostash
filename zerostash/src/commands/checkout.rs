//! `checkout` subcommand

use crate::prelude::*;
use zerostash_files::restore;

#[derive(Command, Debug)]
pub struct Checkout {
    #[clap(flatten)]
    stash: StashArgs,

    #[clap(flatten)]
    options: restore::Options,
}

#[async_trait]
impl AsyncRunnable for Checkout {
    /// Start the application.
    async fn run(&self) {
        let stash = self.stash.open();
        stash.load(stash.index().tree()).unwrap();

        self.options
            .from_iter(&stash, APP.get_worker_threads())
            .await
            .expect("Error extracting data");
    }
}
