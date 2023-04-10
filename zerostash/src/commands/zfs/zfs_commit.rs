//! `zfs commit` subcommand

use std::{
    io::Read,
    process::{Child, ChildStdout, Stdio},
};

use infinitree::Infinitree;
use zerostash_files::{Files, Snapshot};

use crate::prelude::*;

#[derive(Command, Debug)]
pub struct ZfsCommit {
    #[clap(flatten)]
    stash: StashArgs,

    /// commit message to include in the changeset
    #[clap(short = 'm', long)]
    message: Option<String>,

    /// name the snapshot will be stored by
    #[clap(short = 'n', long)]
    name: String,

    /// zfs send arguments to pass
    #[clap(name = "arguments")]
    #[arg(num_args(1..))]
    arguments: Vec<String>,
}

#[async_trait]
impl AsyncRunnable for ZfsCommit {
    /// Start the application.
    async fn run(&self) {
        let mut stash = self.stash.open();
        stash.load(stash.index().snapshots()).unwrap();

        let mut child = execute_command(&self.arguments);
        let stdout = child.stdout.as_mut().expect("failed to open stdout");
        store_stream_from_stdout(&stash, self.name.clone(), stdout);

        stash
            .commit(self.message.clone())
            .expect("failed to write metadata");
        stash.backend().sync().expect("failed to write to storage");
    }
}

fn execute_command(arguments: &[String]) -> Child {
    let mut child = std::process::Command::new("zfs")
        .arg("send")
        .args(arguments)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to execute zfs send");

    let status = child.wait().expect("failed to wait for child process");

    let stderr = child.stderr.as_mut().expect("failed to open stderr");
    if !status.success() {
        let mut err = String::new();
        stderr.read_to_string(&mut err).unwrap();
        panic!("err: {}", err);
    }

    child
}

fn store_stream_from_stdout(stash: &Infinitree<Files>, snapshot: String, stdout: &mut ChildStdout) {
    let snapshots = &stash.index().snapshots;

    if snapshots.get(&snapshot).is_some() {
        panic!("cannot override existing snapshot");
    }

    let writer = stash.storage_writer().unwrap();
    let stream = Snapshot::from_stdout(writer, stdout).expect("failed to capture snapshot");

    snapshots.insert(snapshot, stream);
}
