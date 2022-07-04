use crate::config::Key;
use crate::prelude::*;
use clap::ArgGroup;
use std::path::PathBuf;

#[derive(Command, Debug)]
pub struct Keys {
    #[clap(subcommand)]
    cmd: KeyCommand,
}

#[async_trait]
impl AsyncRunnable for Keys {
    async fn run(&self) {
        self.cmd.run().await
    }
}

// 0s keys generate split --read read.toml --write write.toml
// 0s keys generate password -y -u username --keyfile yubikey.toml
// 0s keys generate password --keyfile k.toml stash_name

// 0s keys change <stash args> stash_name to <stash args>

// 0s keys generate password -e username stash_name
// 0s keys del -e username stash_ref

#[derive(Command, Debug)]
pub enum KeyCommand {
    /// Generate and export new keys to keyfiles
    #[clap(alias = "gen")]
    Generate(Generate),
    /// Change the keys for an existing stash
    #[clap(alias = "ch")]
    Change(Change),
    /// Manage macOS Keychain
    #[clap(alias = "rm")]
    Delete,
}

#[async_trait]
impl AsyncRunnable for KeyCommand {
    async fn run(&self) {
        println!("ye");
    }
}

#[derive(Command, Debug)]
pub struct Generate {
    #[clap(subcommand)]
    cmd: Key,
}

#[derive(Command, Debug)]
pub struct Change {
    #[clap(flatten)]
    from: StashArgs,
    #[clap(subcommand)]
    cmd: ChangeCmd,
}

#[derive(clap::Subcommand, Debug)]
pub enum ChangeCmd {
    To(ChangeTo),
}

#[derive(Command, Debug)]
#[clap(group(
            ArgGroup::new("key")
	        .required(true)
                .args(&["keyfile", "keystring"]),
        ))]
pub struct ChangeTo {
    /// Use a keyfile for the stash
    #[clap(short, long, value_name = "PATH")]
    pub keyfile: Option<PathBuf>,

    /// Use a key specification TOML. Eg: '{ source = "yubikey" }'
    #[clap(short = 'K', value_name = "TOML", long)]
    pub keystring: Option<String>,
}
