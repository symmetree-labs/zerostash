use crate::keygen::Generate;
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
}

#[async_trait]
impl AsyncRunnable for KeyCommand {
    async fn run(&self) {
        use KeyCommand::*;
        match self {
            Generate(g) => g.run().await,
            Change(c) => c.run().await,
        }
    }
}

#[derive(Command, Debug)]
pub struct Change {
    #[clap(flatten)]
    from: StashArgs,
    #[clap(subcommand)]
    cmd: ChangeCmd,
}

#[async_trait]
impl AsyncRunnable for Change {
    async fn run(&self) {
        todo!()
    }
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
