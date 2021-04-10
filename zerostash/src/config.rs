//! Zerostash Config
//!
//! See instructions in `commands.rs` to specify the path to your
//! application's configuration file and/or command-line options
//! for specifying it.

use anyhow::Result;
use serde::{Deserialize, Serialize};

use std::{collections::HashMap, path::PathBuf, sync::Arc};

/// Zerostash Configuration
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ZerostashConfig {
    /// An example configuration section
    #[serde(rename = "stash")]
    stashes: HashMap<String, Stash>,
}

impl Default for ZerostashConfig {
    fn default() -> ZerostashConfig {
        ZerostashConfig {
            stashes: HashMap::new(),
        }
    }
}

/// Describe the configuration for a named stash
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Stash {
    key: Key,
    pub(crate) backend: Backend,
}

impl Stash {
    /// Try to open a stash with the config-stored credentials
    pub fn try_open(&self) -> Result<libzerostash::Stash> {
        let key = {
            use Key::*;
            match &self.key {
                Interactive => ask_credentials()?,
                Plaintext { user, password } => libzerostash::StashKey::open_stash(user, password)?,
            }
        };

        let stash = {
            use Backend::*;
            match &self.backend {
                Filesystem { path } => libzerostash::Stash::new(
                    Arc::new(libzerostash::backends::Directory::new(path)?),
                    key,
                ),
            }
        };

        Ok(stash)
    }
}

/// Ask for credentials on the standard input using [rpassword]
pub fn ask_credentials() -> Result<libzerostash::StashKey> {
    let username = rprompt::prompt_reply_stderr("Username: ")?;
    let password = rpassword::prompt_password_stderr("Password: ")?;
    Ok(libzerostash::StashKey::open_stash(username, password)?)
}

/// Credentials for a stash
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[serde(tag = "source")]
pub enum Key {
    /// Plain text username/password pair
    #[serde(rename = "plaintext")]
    #[allow(missing_docs)]
    Plaintext { user: String, password: String },

    /// Get credentials through other interactive/command line methods
    #[serde(rename = "ask")]
    Interactive,
}

/// Backend configuration
/// This may be specific to the backend type
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[serde(tag = "type")]
pub enum Backend {
    /// Use a directory on a local filesystem
    #[serde(rename = "fs")]
    #[allow(missing_docs)]
    Filesystem { path: String },
}

impl ZerostashConfig {
    /// Path to the configuration directory
    pub fn path() -> PathBuf {
        xdg::BaseDirectories::with_prefix("zerostash")
            .unwrap()
            .place_config_file("config.toml")
            .expect("cannot create configuration directory")
    }

    /// Write the config file to the file system
    pub fn write(&self) -> Result<()> {
        unimplemented!()
    }

    /// Find a stash by name in the config, and return a read-only
    /// reference if found
    pub fn resolve_stash(&self, alias: impl AsRef<str>) -> Option<&Stash> {
        self.stashes.get(alias.as_ref())
    }
}

mod tests {
    #[test]
    fn can_parse_config() {
        use super::ZerostashConfig;
        use abscissa_core::Config;

        ZerostashConfig::load_toml(
            r#"
[stash.first]
key = { source = "plaintext", user = "123", password = "123"}
backend = { type = "fs", path = "/path/to/stash" }

[stash.second]
key = { source = "ask"}
backend = { type = "fs", path = "/path/to/stash" }
"#,
        )
        .unwrap();
    }
}
