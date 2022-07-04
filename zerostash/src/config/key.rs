use super::{KeyToSource, Result};
use crate::prelude::Command;
use infinitree::keys::KeySource;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Credentials for a stash
#[derive(Command, Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[serde(tag = "source")]
pub enum Key {
    /// Plain text username/password pair
    #[serde(rename = "plaintext")]
    Plaintext(super::SymmetricKey),

    /// User/Password pair + 2FA
    #[serde(rename = "yubikey")]
    Yubikey(super::YubikeyCRKey),

    /// Use a different key for reading archives and appending
    #[serde(rename = "split_key")]
    #[clap(name = "split_key")]
    SplitKeyStorage(super::SplitKeyStorage),

    #[cfg(target_os = "macos")]
    #[serde(rename = "macos_keychain")]
    #[clap(name = "macos_keychain")]
    MacOsKeychain(super::KeychainCredentials),

    /// Get credentials through other interactive/command line methods
    #[serde(rename = "ask")]
    #[clap(skip)]
    Interactive,

    /// Plain text username/password pair
    #[serde(rename = "file")]
    #[allow(missing_docs)]
    #[clap(skip)]
    KeyFile { path: PathBuf },
}

impl Key {
    // On non-macos the parameter will generate a warning.
    #[allow(unused)]
    pub(super) fn get_credentials(self, stash: &str) -> Result<KeySource> {
        match self {
            Self::KeyFile { path } => {
                let contents = std::fs::read_to_string(path)?;
                let keys: Key = toml::from_str(&contents)?;

                // this is technically recursion, it may be an ouroboros
                keys.get_credentials(stash)
            }
            Self::Interactive => super::SymmetricKey::default().to_keysource(stash),
            Self::Plaintext(k) => k.to_keysource(stash),
            Self::Yubikey(k) => k.to_keysource(stash),
            Self::SplitKeyStorage(k) => k.to_keysource(stash),
            #[cfg(target_os = "macos")]
            Self::MacOsKeychain(k) => k.to_keysource(stash),
        }
    }
}
