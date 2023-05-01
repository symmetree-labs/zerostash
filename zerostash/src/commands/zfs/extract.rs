//! `zfs extract` subcommand

use std::{
    io::Read,
    process::{Child, ChildStdin, Stdio},
};

use infinitree::Infinitree;
use zerostash_files::Files;

use crate::prelude::*;

#[derive(Command, Debug)]
pub struct ZfsExtract {
    #[clap(flatten)]
    stash: StashArgs,

    /// Name of the stored snapshot to restore
    #[clap(short = 'n', long)]
    name: String,

    /// Arguments to `zfs recv`
    #[clap(name = "arguments")]
    #[arg(num_args(1..))]
    arguments: Vec<String>,
}

#[async_trait]
impl AsyncRunnable for ZfsExtract {
    /// Start the application.
    async fn run(&self) {
        let stash = self.stash.open();
        stash.load(stash.index().zfs_snapshots()).unwrap();

        let mut child = execute_command(&self.arguments);
        let stdin = child.stdin.as_mut().expect("failed to open stdin");
        write_stream_to_stdin(&stash, &self.name, stdin);

        let status = child.wait().expect("failed to wait for child process");

        let stderr = child.stderr.as_mut().expect("failed to open stderr");
        if !status.success() {
            let mut err = String::new();
            stderr.read_to_string(&mut err).unwrap();
            panic!("err: {}", err);
        }
    }
}

fn execute_command(arguments: &[String]) -> Child {
    std::process::Command::new("zfs")
        .arg("receive")
        .args(arguments)
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to execute zfs receive")
}

fn write_stream_to_stdin(stash: &Infinitree<Files>, snapshot: &str, stdin: &mut ChildStdin) {
    if let Some(stream) = stash.index().zfs_snapshots.get(snapshot) {
        let reader = stash.storage_reader().unwrap();
        abscissa_tokio::tokio::task::block_in_place(|| stream.to_stdin(reader, stdin))
            .expect("failed to write to stdin");
    } else {
        panic!("snapshot not stashed");
    }
}
