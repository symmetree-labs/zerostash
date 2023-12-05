use crate::config::Key;
use crate::keygen::{GenKeyCmd, Generate, GenerateKey};
use crate::prelude::*;
use anyhow::{anyhow, bail};
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
        let stash_cfg = self.from.parse_stash();
        let old_key = self.from.key().unwrap_or_else(|| stash_cfg.key.clone());

        let key = self
            .cmd
            .key(old_key, &stash_cfg.alias)
            .unwrap_or_else(|_| fatal_error("Invalid new key"));

        let stash = stash_cfg.try_open(Some(key)).unwrap();
        if stash.reseal().is_err() {
            fatal_error("Failed to change key");
        }
    }
}

#[derive(clap::Subcommand, Debug, Clone)]
pub enum ChangeCmd {
    Toml(ChangeTo),

    #[clap(flatten)]
    Generate(GenKeyCmd),
}

impl ChangeCmd {
    fn key(&self, old_key: Key, stash: impl Into<String>) -> anyhow::Result<Key> {
        let new_key = match self {
            Self::Toml(ch) => ch.get_key()?,
            ChangeCmd::Generate(cmd) => {
                let g = Generate {
                    stash: stash.into(),
                    cmd: cmd.clone(),
                };

                let keys = g.clone().cmd.generate(&g)?;
                let effective = keys
                    .get(0)
                    .ok_or_else(|| anyhow!("Could not generate key!"))
                    .map(|w| w.obj.clone())?;

                for mut writer in keys {
                    if let Key::SplitKeyStorage(new) = &mut writer.obj {
                        if let Key::SplitKeyStorage(old) = &old_key {
                            new.keys.write = old.keys.write.clone();

                            if new.keys.read.is_some() && old.keys.read.is_none() {
                                continue;
                            } else {
                                new.keys.read = old.keys.read.clone();
                            }
                        } else {
                            bail!("Invalid new key");
                        }
                    }

                    writer.write();
                }

                effective
            }
        };

        Ok(old_key.change_to(new_key))
    }
}

#[derive(Command, Debug, Clone)]
#[clap(group(
            ArgGroup::new("key")
	        .required(true)
                .args(&["keyfile", "keystring"]),
        ))]
/// Read the key configuration from a TOML file
pub struct ChangeTo {
    /// Use a keyfile for the stash
    #[clap(short, long, value_name = "PATH")]
    pub keyfile: Option<PathBuf>,

    /// Use a key specification TOML. Eg: '{ source = "yubikey" }'
    #[clap(short = 'K', value_name = "TOML", long)]
    pub keystring: Option<String>,
}

impl ChangeTo {
    fn get_key(&self) -> anyhow::Result<Key> {
        if let Some(ref path) = self.keyfile {
            return Ok(Key::KeyFile { path: path.clone() });
        }

        if let Some(ref key) = self.keystring {
            return Ok(toml::from_str::<Key>(key)?);
        }

        unreachable!()
    }
}
