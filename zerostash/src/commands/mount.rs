//! `mount` subcommand

use crate::prelude::*;

#[derive(Command, Debug)]
pub struct Mount {
    #[clap(flatten)]
    stash: StashArgs,

    #[clap(flatten)]
    options: zerostash_files::restore::Options,

    /// The location the filesytem will be mounted on
    #[clap(short = 'T', long = "target")]
    mount_point: String,
}

#[cfg(unix)]
#[async_trait]
impl AsyncRunnable for Mount {
    /// Start the application.
    async fn run(&self) {
        let stash = self.stash.open();
        let threads = APP.get_worker_threads();

        if let Err(e) =
            zerostash_fuse::mount::mount(stash, &self.options, &self.mount_point, threads)
        {
            panic!("Error = {}", e)
        }
    }
}
