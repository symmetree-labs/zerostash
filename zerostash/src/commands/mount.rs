//! `mount` subcommand

use crate::{migration::migration, prelude::*};

#[derive(Command, Debug)]
pub struct Mount {
    #[clap(flatten)]
    stash: StashArgs,

    #[clap(flatten)]
    options: zerostash_files::restore::Options,

    /// The location the filesytem will be mounted on
    #[clap(short = 'T', long = "target")]
    mount_point: String,

    /// Mounts the filesystem read-write
    #[clap(short = 'w', long = "read-write")]
    read_write: bool,
}

#[cfg(unix)]
#[async_trait]
impl AsyncRunnable for Mount {
    /// Start the application.
    async fn run(&self) {
        let mut stash = self.stash.open();
        let threads = APP.get_worker_threads();
        stash.load(stash.index().tree()).unwrap();
        stash.load(stash.index().files()).unwrap();
        migration(&mut stash);

        if let Err(e) = zerostash_fuse::mount::mount(
            stash,
            &self.options,
            &self.mount_point,
            threads,
            self.read_write,
        )
        .await
        {
            panic!("Error = {}", e)
        }
    }
}
