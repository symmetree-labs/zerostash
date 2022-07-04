use super::{KeyToSource, Result};
use infinitree::keys::KeySource;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Credentials for a stash
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[serde(tag = "source")]
pub enum Key {
    /// Plain text username/password pair
    #[serde(rename = "plaintext")]
    Plaintext(super::SymmetricKey),

    /// 2 factor authentication with a Yubikey
    #[serde(rename = "yubikey")]
    Yubikey(super::YubikeyCRKey),

    /// Use a different key for reading archives and appending
    #[serde(rename = "split_key")]
    SplitKeyStorage(super::SplitKeyStorage),

    /// Get credentials through other interactive/command line methods
    #[serde(rename = "ask")]
    Interactive,

    /// Plain text username/password pair
    #[serde(rename = "file")]
    #[allow(missing_docs)]
    KeyFile { path: PathBuf },
}

impl KeyToSource for Key {
    fn to_keysource(self, stash: &str) -> Result<KeySource> {
        match self {
            Self::KeyFile { path } => {
                let contents = std::fs::read_to_string(path)?;
                let keys: Key = toml::from_str(&contents)?;

                // this is technically recursion, it may be an ouroboros
                keys.to_keysource(stash)
            }
            Self::Interactive => super::SymmetricKey::default().to_keysource(stash),
            Self::Plaintext(k) => k.to_keysource(stash),
            Self::Yubikey(k) => k.to_keysource(stash),
            Self::SplitKeyStorage(k) => k.to_keysource(stash),
        }
    }
}
