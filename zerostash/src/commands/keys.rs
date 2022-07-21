use crate::config::{Key, KeyToSource};
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
        let old_key = self
            .from
            .key()
            .or_else(|| APP.config().resolve_stash(&self.from.stash).map(|s| s.key))
            .unwrap_or_default();

        let new_key = self
            .cmd
            .key()
            .unwrap_or_else(|_| fatal_error("Invalid new key"));

        let key = old_key.change_to(new_key);

        let mut stash = self.from.try_open(Some(key));
        if stash.reseal().is_err() {
            fatal_error("Failed to change key");
        }
    }
}

#[derive(clap::Subcommand, Debug, Clone)]
pub enum ChangeCmd {
    To(ChangeTo),
}

impl ChangeCmd {
    fn key(&self) -> anyhow::Result<Key> {
        match self {
            Self::To(ch) => {
                if ch.interactive {
                    return Ok(Key::Interactive);
                }

                if let Some(ref path) = ch.keyfile {
                    return Ok(Key::KeyFile { path: path.clone() });
                }

                if let Some(ref key) = ch.keystring {
                    return Ok(toml::from_str::<Key>(key)?);
                }

                unreachable!()
            }
        }
    }
}

#[derive(Command, Debug, Clone)]
#[clap(group(
            ArgGroup::new("key")
	        .required(true)
                .args(&["keyfile", "keystring", "interactive"]),
        ))]
pub struct ChangeTo {
    /// Use a keyfile for the stash
    #[clap(short, long, value_name = "PATH")]
    pub keyfile: Option<PathBuf>,

    /// Use a key specification TOML. Eg: '{ source = "yubikey" }'
    #[clap(short = 'K', value_name = "TOML", long)]
    pub keystring: Option<String>,

    /// Use a key specification TOML. Eg: '{ source = "yubikey" }'
    #[clap(short = 'i', long)]
    pub interactive: bool,
}

impl KeyToSource for ChangeCmd {
    type Target = infinitree::Key;
    fn to_keysource(self, stash: &str) -> anyhow::Result<Self::Target> {
        self.key()?.to_keysource(stash)
    }
}
