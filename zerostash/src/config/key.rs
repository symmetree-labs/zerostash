use super::{KeyToSource, Result};
use anyhow::bail;
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, sync::Arc};

/// Credentials for a stash
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[serde(tag = "source")]
#[derive(Default)]
pub enum Key {
    /// Plain text username/password pair
    #[serde(rename = "plaintext")]
    Userpass(super::SymmetricKey),

    /// 2 factor authentication with a Yubikey
    #[serde(rename = "yubikey")]
    Yubikey(super::YubikeyCRKey),

    /// Use a different key for reading archives and appending
    #[serde(rename = "split_key")]
    SplitKeyStorage(super::SplitKeyStorage),

    /// Get credentials through other interactive/command line methods
    #[serde(rename = "ask")]
    #[default]
    Interactive,

    /// Plain text username/password pair
    #[serde(rename = "file")]
    #[allow(missing_docs)]
    KeyFile { path: PathBuf },

    /// Creates a `ChangeKey` structure
    #[serde(skip)]
    ChangeTo { old: Box<Key>, new: Box<Key> },
}

impl Key {
    pub(crate) fn change_to(self, new: Key) -> Key {
        Key::ChangeTo {
            old: Box::new(self),
            new: Box::new(new),
        }
    }
}

macro_rules! change_key {
    ($stash:ident, $old:expr, $new:expr) => {
        Arc::new(infinitree::crypto::ChangeHeaderKey::swap_on_seal(
            $old.to_keysource($stash)?,
            $new.to_keysource($stash)?,
        ))
    };
}

macro_rules! old {
    () => {{
        println!("Current credentials for the stash:\n");
        super::SymmetricKey::default()
    }};
}

macro_rules! new {
    () => {{
        println!("New credentials:\n");
        super::SymmetricKey::default()
    }};
}

impl KeyToSource for Key {
    type Target = infinitree::Key;

    fn to_keysource(self, stash: &str) -> Result<infinitree::Key> {
        Ok(match self {
            Self::KeyFile { path } => {
                let contents = std::fs::read_to_string(path)?;
                let keys: Key = toml::from_str(&contents)?;

                // this is technically recursion, it may be an ouroboros
                keys.to_keysource(stash)?
            }
            Self::Interactive => Arc::new(super::SymmetricKey::default().to_keysource(stash)?),
            Self::Userpass(k) => Arc::new(k.to_keysource(stash)?),
            Self::Yubikey(k) => Arc::new(k.to_keysource(stash)?),
            Self::SplitKeyStorage(k) => Arc::new(k.to_keysource(stash)?),

            Self::ChangeTo { old, new } => match (*old, *new) {
                (Key::Interactive, Key::Interactive) => {
                    change_key!(stash, old!(), new!())
                }
                (Key::Interactive, Key::Userpass(new)) => change_key!(stash, old!(), new),
                (Key::Interactive, Key::Yubikey(new)) => change_key!(stash, old!(), new),

                (Key::Userpass(old), Key::Interactive) => change_key!(stash, old, new!()),
                (Key::Userpass(old), Key::Userpass(new)) => change_key!(stash, old, new),
                (Key::Userpass(old), Key::Yubikey(new)) => change_key!(stash, old, new),

                (Key::Yubikey(old), Key::Interactive) => change_key!(stash, old, new!()),
                (Key::Yubikey(old), Key::Userpass(new)) => change_key!(stash, old, new),
                (Key::Yubikey(old), Key::Yubikey(new)) => change_key!(stash, old, new),

                (Key::SplitKeyStorage(old), Key::SplitKeyStorage(new)) => {
                    change_key!(stash, old, new)
                }
                _ => bail!("Old and new keys are incompatible!"),
            },
        })
    }
}
