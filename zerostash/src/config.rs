//! Zerostash Config
//!
//! See instructions in `commands.rs` to specify the path to your
//! application's configuration file and/or command-line options
//! for specifying it.

use crate::{application::APP, prelude::Stash as InfiniStash};
use abscissa_core::Application;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::PathBuf, str::FromStr, sync::Arc};

mod crypto_box_keys;
pub use crypto_box_keys::*;
mod symmetric_key;
pub use symmetric_key::*;
mod yubikey;
pub use yubikey::*;

mod key;
pub use key::*;
mod backend;
pub use backend::*;

pub trait KeyToSource {
    type Target;
    fn to_keysource(self, _stash_name: &str) -> Result<Self::Target>;
}

/// Zerostash Configuration
#[derive(Default, Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ZerostashConfig {
    /// An example configuration section
    #[serde(rename = "stash", default)]
    stashes: HashMap<String, Stash>,
}

/// Describe the configuration for a named stash
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Stash {
    /// Key descriptor to use while opening the stash
    pub key: Key,
    /// Backend configuration for the stash
    pub backend: Backend,

    /// Name as referenced by the user. We can't deserialize this.
    /// However, when reading the config, `resolve_stash` will populate it.
    #[serde(skip)]
    pub alias: String,
}

impl FromStr for Stash {
    type Err = anyhow::Error;

    fn from_str(name: &str) -> Result<Self, Self::Err> {
        let mut stash = match APP.config().resolve_stash(name) {
            Some(stash) => stash,
            None => Stash {
                backend: name.parse()?,
                alias: name.to_string(),
                key: Default::default(),
            },
        };

        if let Backend::Filesystem { path } = &stash.backend {
            stash.alias = path.clone();
        };

        Ok(stash)
    }
}

impl Stash {
    fn get_locators(
        &self,
        override_key: Option<Key>,
    ) -> Result<(Arc<dyn infinitree::backends::Backend>, infinitree::Key)> {
        let backend = self.backend.to_infinitree()?;

        // This is to use absolute paths in the FS.
        let keysource = match override_key {
            Some(key) => key,
            None => self.key.clone(),
        }
        .to_keysource(&self.alias)?;

        Ok((backend, keysource))
    }

    /// Try to open a stash with the config-stored credentials
    pub fn try_open(&self, override_key: Option<Key>) -> Result<InfiniStash> {
        let (backend, key) = self.get_locators(override_key)?;
        InfiniStash::open(backend, key)
    }

    pub fn open_or_new(&self, override_key: Option<Key>) -> Result<InfiniStash> {
        let (backend, key) = self.get_locators(override_key)?;
        let stash = InfiniStash::open(backend.clone(), key.clone())
            .or_else(|_| InfiniStash::empty(backend, key))?;

        Ok(stash)
    }
}

impl ZerostashConfig {
    /// Path to the configuration directory
    #[cfg(unix)]
    pub fn path() -> PathBuf {
        xdg::BaseDirectories::with_prefix("zerostash")
            .unwrap()
            .place_config_file("config.toml")
            .expect("cannot create configuration directory")
    }

    /// Path to the configuration directory
    #[cfg(windows)]
    pub fn path() -> PathBuf {
        let mut p = dirs::home_dir().expect("cannot find home directory");

        p.push(".zerostash");
        std::fs::create_dir_all(&p).expect("failed to create config dir");

        p.push("config.toml");
        p
    }

    /// Write the config file to the file system
    pub fn write(&self) -> Result<()> {
        unimplemented!()
    }

    /// Find a stash by name in the config, and return a read-only
    /// reference if found
    pub fn resolve_stash(&self, alias: impl AsRef<str>) -> Option<Stash> {
        match self.stashes.get(alias.as_ref()).cloned() {
            Some(mut stash) => {
                stash.alias = alias.as_ref().to_string();
                Some(stash)
            }
            None => None,
        }
    }
}

#[cfg(test)]
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

[stash.yubikey]
key = { source = "yubikey", user = "123", password = "123", slot = "slot1", key = "hmac1" }
backend = { type = "fs", path = "/path/to/stash" }

[stash.yubikey2]
key = { source = "yubikey", user = "123", password = "123" }
backend = { type = "fs", path = "/path/to/stash" }

[stash.writeonly]
key = { source = "split_key", user = "123", password = "123", write = "p0s-1xqcrzvfjxgenxdp5x56nvd3hxuurswfev9skycnrvdjxget9venq9sue6u"}
backend = { type = "fs", path = "/path/to/stash" }

[stash.split_readwrite]
backend = { type = "fs", path = "/path/to/stash" }
[stash.split_readwrite.key]
source = "split_key"
user = "123"
password = "123"
write = "p0s-1xqcrzvfjxgenxdp5x56nvd3hxuurswfev9skycnrvdjxget9venq9sue6u"
read = "s0s-1xqcrzvfjxgenxdp5x56nvd3hxuurswfev9skycnrvdjxget9venqn52utr"

[stash.keyfile]
key = { source = "file", path = "./example_keyfile.toml" }
backend = { type = "fs", path = "/path/to/stash" }


[stash.s3]
key = { source = "ask" }
backend = { type = "s3", bucket = "test_bucket", region = { name = "us-east-1" }, keys = ["access_key_id", "secret_key"] }

[stash.s3_env_key]
key = { source = "ask" }
backend = { type = "s3", bucket = "test_bucket", region = { name = "us-east-1" } }

[stash.s3_cached]
key = { source = "ask" }

[stash.s3_cached.backend]
type = "fs_cache"
path = "/path_to_stash"
max_size_mb = 1024

[stash.s3_cached.backend.upstream]
type = "s3"
bucket = "test_bucket"
region = { name = "custom", details = { endpoint = "https://127.0.0.1:8080/", "region" = "" }}
"#,
        )
        .unwrap();
    }

    #[test]
    fn can_load_empty() {
        use super::ZerostashConfig;
        use abscissa_core::Config;

        ZerostashConfig::load_toml(r#""#).unwrap();
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn can_load_keychain_config() {
        use super::ZerostashConfig;
        use abscissa_core::Config;

        ZerostashConfig::load_toml(
            r#"
[stash.macos_keychain]
key = { source = "plaintext", user = "user@example.com", keychain = true }
backend = { type = "fs", path = "/path/to/stash" }
"#,
        )
        .unwrap();
    }

    #[test]
    fn load_keyfile() {
        use super::{Key, KeyToSource};
        use std::path::PathBuf;

        let mut path: PathBuf = std::env::var("CARGO_MANIFEST_DIR").unwrap().into();
        assert!(path.pop());
        path.push("keyfile.toml.example");

        let key = Key::KeyFile { path };
        key.to_keysource("stash name").unwrap();
    }

    #[test]
    fn can_load_example() {
        use super::ZerostashConfig;
        use abscissa_core::Config;
        use std::path::PathBuf;

        let mut path: PathBuf = std::env::var("CARGO_MANIFEST_DIR").unwrap().into();
        assert!(path.pop());
        path.push("config.toml.example");
        println!("{:?}", std::env::vars().collect::<Vec<_>>());

        let example = std::fs::read_to_string(path).unwrap();
        ZerostashConfig::load_toml(example).unwrap();
    }

    #[test]
    fn can_parse_s3_url() {
        use super::Backend;
        use infinitree_backends::Region;

        assert_eq!(
            "s3://access:secret@us-east-1#/bucket/path"
                .parse::<Backend>()
                .unwrap(),
            Backend::S3 {
                bucket: "bucket/path".into(),
                region: Region::UsEast1,
                keys: Some(("access".into(), "secret".into()))
            }
        );

        assert_eq!(
            "s3://us-east-1#/bucket/path".parse::<Backend>().unwrap(),
            Backend::S3 {
                bucket: "bucket/path".into(),
                region: Region::UsEast1,
                keys: None
            }
        );

        assert_eq!(
            "s3://us-east-1#server.com/bucket/path"
                .parse::<Backend>()
                .unwrap(),
            Backend::S3 {
                bucket: "bucket/path".into(),
                region: Region::Custom {
                    region: "us-east-1".into(),
                    endpoint: "server.com".into()
                },
                keys: None
            }
        );

        assert_eq!(
            "s3://access:secret@server.com/bucket/path"
                .parse::<Backend>()
                .unwrap(),
            Backend::S3 {
                bucket: "bucket/path".into(),
                region: Region::Custom {
                    region: "".into(),
                    endpoint: "server.com".into()
                },
                keys: Some(("access".into(), "secret".into()))
            }
        );

        assert_eq!(
            "s3://accesskey:secret+key/=@us-east-1#server.com/bucket/path"
                .parse::<Backend>()
                .unwrap(),
            Backend::S3 {
                bucket: "bucket/path".into(),
                region: Region::Custom {
                    region: "us-east-1".into(),
                    endpoint: "server.com".into()
                },
                keys: Some(("accesskey".into(), "secret+key/=".into()))
            }
        )
    }

    #[test]
    fn no_scheme_gets_file_backend() {
        use super::Backend;

        if let Backend::Filesystem { .. } = "example/path".parse::<Backend>().unwrap() {
            assert!(true);
        } else {
            assert!(false);
        }
    }
}
