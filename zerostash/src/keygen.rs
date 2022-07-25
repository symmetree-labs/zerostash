use crate::{
    config::{Key, YubikeyCRConfig},
    prelude::*,
};
use anyhow::Result;
use serde::Serialize;
use std::path::PathBuf;

pub trait GenerateKey
where
    Self: Sized,
{
    fn generate(self, gen: &Generate) -> Result<Vec<WriteToFile<Key>>>;
}

pub struct WriteToFile<T> {
    pub obj: T,
    pub file: PathBuf,
}

impl<T: Serialize> WriteToFile<T> {
    pub fn write(&self) {
        let bytes = toml::ser::to_vec(&self.obj).expect("Can't serialize native object");
        std::fs::write(&self.file, &bytes).expect("Can't write to output file");
    }
}

#[derive(Command, Debug, Clone)]
pub struct Generate {
    /// Name of the stash
    pub stash: String,

    /// Type of key to generate
    #[clap(subcommand)]
    pub cmd: GenKeyCmd,
}

#[async_trait]
impl AsyncRunnable for Generate {
    async fn run(&self) {
        for file in self
            .cmd
            .clone()
            .generate(self)
            .expect("Generation gone wrong")
        {
            file.write()
        }
    }
}

/// Credentials for a stash
#[derive(Command, Clone, Debug)]
pub enum GenKeyCmd {
    /// Generate a username/password pair
    Userpass(SymmetricKey),

    /// Generate a username/password pair with 2 factor authentication using a Yubikey
    Yubikey(YubikeyCRKey),

    /// Generate split keys with read/write and write-only permissions.
    /// Changing the read/write keys is NOT supported!
    #[clap(name = "split_key")]
    SplitKeyStorage(SplitKeyStorage),
}

impl GenerateKey for GenKeyCmd {
    fn generate(self, gen: &Generate) -> Result<Vec<WriteToFile<Key>>> {
        match self {
            Self::Userpass(k) => k.generate(gen),
            Self::Yubikey(k) => k.generate(gen),
            Self::SplitKeyStorage(k) => k.generate(gen),
        }
    }
}

#[derive(clap::Args, Default, Clone, Debug)]
pub struct SymmetricKey {
    /// Username
    #[clap(short, long)]
    pub user: String,

    /// Use macOS Keychain for storing the password
    #[clap(short = 'e', long = "keychain")]
    #[cfg(target_os = "macos")]
    pub keychain: bool,

    #[clap(short = 'f', long)]
    pub keyfile: PathBuf,
}

impl SymmetricKey {
    #[cfg(target_os = "macos")]
    fn fill_random(self, stash: &str) -> Result<crate::config::SymmetricKey> {
        crate::config::SymmetricKey {
            user: Some(self.user.into()),
            keychain: self.keychain,
            ..Default::default()
        }
        .fill_random(stash)
    }

    #[cfg(not(target_os = "macos"))]
    fn fill_random(self, stash: &str) -> Result<crate::config::SymmetricKey> {
        crate::config::SymmetricKey {
            user: Some(self.user.into()),
            ..Default::default()
        }
        .fill_random(stash)
    }
}

impl GenerateKey for SymmetricKey {
    fn generate(self, gen: &Generate) -> Result<Vec<WriteToFile<Key>>> {
        let file = self.keyfile.clone();
        let key: crate::config::SymmetricKey = self.fill_random(&gen.stash)?;

        Ok(vec![WriteToFile {
            file,
            obj: Key::Userpass(key),
        }])
    }
}

#[derive(clap::Args, Debug, Clone)]
pub struct YubikeyCRKey {
    #[clap(flatten)]
    pub credentials: SymmetricKey,

    #[clap(flatten)]
    pub config: YubikeyCRConfig,
}

impl GenerateKey for YubikeyCRKey {
    fn generate(self, gen: &Generate) -> Result<Vec<WriteToFile<Key>>> {
        let file = self.credentials.keyfile.clone();
        let key = self.credentials.fill_random(&gen.stash)?;

        Ok(vec![WriteToFile {
            file,
            obj: Key::Yubikey(crate::config::YubikeyCRKey {
                credentials: key,
                config: self.config,
            }),
        }])
    }
}

#[derive(clap::Args, Debug, Clone)]
pub struct SplitKeyStorage {
    /// Username
    #[clap(short, long)]
    pub user: String,

    /// Use macOS Keychain for storing the password
    #[clap(short = 'e', long = "keychain")]
    #[cfg(target_os = "macos")]
    pub keychain: bool,

    #[clap(short = 's', long)]
    read_keyfile: PathBuf,

    #[clap(short = 'p', long)]
    write_keyfile: PathBuf,
}

impl GenerateKey for SplitKeyStorage {
    fn generate(self, gen: &Generate) -> Result<Vec<WriteToFile<Key>>> {
        let (rw, wo) = crate::config::SplitKeys::default().split();

        let key: crate::config::SymmetricKey = SymmetricKey {
            user: self.user,
            keychain: self.keychain,
            ..Default::default()
        }
        .fill_random(&gen.stash)?;

        Ok(vec![(wo, self.write_keyfile), (rw, self.read_keyfile)]
            .into_iter()
            .map(|(k, file)| WriteToFile {
                file,
                obj: Key::SplitKeyStorage(crate::config::SplitKeyStorage {
                    credentials: key.clone(),
                    keys: k,
                }),
            })
            .collect())
    }
}
