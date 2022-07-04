use crate::{
    config::{Key, YubikeyCRConfig},
    prelude::*,
};
use anyhow::Result;
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

#[derive(Command, Debug)]
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
            let bytes = toml::ser::to_vec(&file.obj).expect("Can't serialize native object");
            std::fs::write(&file.file, &bytes).expect("Can't write to output file");
        }
    }
}

/// Credentials for a stash
#[derive(Command, Clone, Debug)]
pub enum GenKeyCmd {
    /// Plain text username/password pair
    Plaintext(SymmetricKey),

    /// 2 factor authentication with a Yubikey
    Yubikey(YubikeyCRKey),

    /// Use a different key for reading archives and appending
    #[clap(name = "split_key")]
    SplitKeyStorage(SplitKeyStorage),
}

impl GenerateKey for GenKeyCmd {
    fn generate(self, gen: &Generate) -> Result<Vec<WriteToFile<Key>>> {
        match self {
            Self::Plaintext(k) => k.generate(gen),
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
    fn fill_random(self, stash: &str) -> Result<crate::config::SymmetricKey> {
        crate::config::SymmetricKey {
            user: Some(self.user.into()),
            keychain: self.keychain,
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
            obj: Key::Plaintext(key),
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
