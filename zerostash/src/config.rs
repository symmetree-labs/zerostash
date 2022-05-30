//! Zerostash Config
//!
//! See instructions in `commands.rs` to specify the path to your
//! application's configuration file and/or command-line options
//! for specifying it.

use anyhow::{Context, Result};
use infinitree::backends::Region;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, num::NonZeroUsize, path::PathBuf, str::FromStr, sync::Arc};

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
}

impl Stash {
    /// Try to open a stash with the config-stored credentials
    pub fn try_open(&self, name: &str) -> Result<crate::Stash> {
        let (user, pw) = self.key.get_credentials(name)?;

        let key = || infinitree::Key::from_credentials(&user, &pw);
        let backend = self.backend.to_infinitree()?;

        let stash = crate::Stash::open(backend.clone(), key()?)
            .unwrap_or_else(move |_| crate::Stash::empty(backend, key().unwrap()).unwrap());
        Ok(stash)
    }
}

/// Contents of a key file
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct KeyFile {
    user: String,
    password: String,
}

/// Credentials for a stash
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[serde(tag = "source")]
pub enum Key {
    /// Plain text username/password pair
    #[serde(rename = "plaintext")]
    #[allow(missing_docs)]
    Plaintext(KeyFile),

    /// Get credentials through other interactive/command line methods
    #[serde(rename = "ask")]
    Interactive,

    /// Plain text username/password pair
    #[serde(rename = "file")]
    #[allow(missing_docs)]
    KeyFile { path: PathBuf },

    #[cfg(target_os = "macos")]
    #[serde(rename = "macos_keychain")]
    MacOsKeychain { user: String },
}

impl Key {
    // On non-macos the parameter will generate a warning.
    #[allow(unused)]
    fn get_credentials(&self, stash: &str) -> Result<(String, String)> {
        match self {
            Self::Interactive => ask_credentials(),
            Self::Plaintext(KeyFile { user, password }) => {
                Ok((user.to_string(), password.to_string()))
            }
            Self::KeyFile { path } => {
                let contents = std::fs::read_to_string(path)?;
                let keys: KeyFile = toml::from_str(&contents)?;
                Ok((keys.user, keys.password))
            }
            #[cfg(target_os = "macos")]
            Self::MacOsKeychain { user } => {
                let service_name = "dev.symmetree.zerostash";
                let account_name = format!("{}#:0s:#{}", stash, user);

                let pass = security_framework::passwords::get_generic_password(
                    service_name,
                    &account_name,
                )
                .map(|pass| String::from_utf8_lossy(&pass).to_string())
                .unwrap_or_else(|_| {
                    println!(
                        "Keychain entry not found! Please enter the password to save in Keychain!"
                    );
                    let pw = rpassword::prompt_password("Password: ").expect("Invalid password");

                    security_framework::passwords::set_generic_password(
                        service_name,
                        &account_name,
                        pw.as_bytes(),
                    )
                    .expect("Failed to add password to keychain!");

                    pw
                });

                Ok((user.clone(), pass))
            }
        }
    }
}

/// Ask for credentials on the standard input using [rpassword]
pub fn ask_credentials() -> Result<(String, String)> {
    let username = rprompt::prompt_reply_stderr("Username: ")?;
    let password = rpassword::prompt_password("Password: ")?;
    Ok((username, password))
}

/// Backend configuration
/// This may be specific to the backend type
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[serde(tag = "type")]
pub enum Backend {
    /// Use a directory on a local filesystem
    #[serde(rename = "fs")]
    #[allow(missing_docs)]
    Filesystem { path: String },

    /// Descriptor for S3 connection.
    #[serde(rename = "s3")]
    S3 {
        /// name of the bucket
        bucket: String,

        /// May be "protocol://fqdn" syntax.
        /// Supports AWS, DigitalOcean, Yandex, WasabiSys canonical names
        region: Region,

        /// ("access_key_id", "secret_access_key")
        keys: Option<(String, String)>,
    },

    /// Cache files in a local directory, up to `max_size` in size
    /// You will typically want this to be larger than the index size.
    #[serde(rename = "fs_cache")]
    FsCache {
        /// Max size of local cache
        max_size_mb: NonZeroUsize,
        /// Where to store local files
        path: String,
        /// Long-term backend
        upstream: Box<Backend>,
    },
}

impl Backend {
    fn to_infinitree(&self) -> Result<Arc<dyn infinitree::Backend>> {
        use Backend::*;

        let backend: Arc<dyn infinitree::Backend> = match self {
            Filesystem { path } => infinitree::backends::Directory::new(path)?,
            S3 {
                bucket,
                region,
                keys,
            } => {
                use infinitree::backends::{Credentials, S3};

                match keys {
                    Some((access_key, secret_key)) => S3::with_credentials(
                        region.clone(),
                        bucket,
                        Credentials::new(access_key, secret_key),
                    ),
                    None => S3::new(region.clone(), bucket),
                }
                .context("Failed to connect to S3")?
            }
            FsCache {
                max_size_mb,
                path,
                upstream,
            } => infinitree::backends::Cache::new(
                path,
                NonZeroUsize::new(max_size_mb.get() * 1024 * 1024)
                    .expect("Deserialization should have failed if `max_size_mb` is 0"),
                upstream.to_infinitree()?,
            )?,
        };

        Ok(backend)
    }
}

impl FromStr for Backend {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.split_once("://") {
            Some(("s3", url)) => {
                let re = regex::Regex::new(
                    r"^((?P<akey>[a-zA-Z0-9]+):(?P<skey>[a-zA-Z0-9/+=]+)@)?((?P<region>[0-9a-z-]+)#)?(?P<host>[a-zA-Z0-9.-]+)?/(?P<bucketpath>[a-zA-Z0-9./_-]+)?$",
                )
                    .expect("syntactically correct");

                let caps = re.captures(url).context("invalid S3 url")?;
                let akey = caps.name("akey");
                let skey = caps.name("skey");
                let region_name = caps.name("region");
                let host = caps.name("host");
                let bucket = caps
                    .name("bucketpath")
                    .context("no s3 bucket provided")?
                    .as_str()
                    .to_string();

                let region = match (region_name, host) {
                    (Some(r), Some(h)) => Region::Custom {
                        region: r.as_str().into(),
                        endpoint: h.as_str().into(),
                    },
                    (Some(r), None) => r.as_str().parse().context("invalid region name")?,
                    (None, Some(h)) => h.as_str().parse().context("invalid hostname")?,
                    (None, None) => anyhow::bail!("invalid url: no hostname or region"),
                };

                let keys = match (akey, skey) {
                    (Some(a), Some(s)) => Some((a.as_str().to_string(), s.as_str().to_string())),
                    _ => None,
                };

                Ok(Backend::S3 {
                    bucket,
                    region,
                    keys,
                })
            }
            Some(_) => anyhow::bail!("protocol not supported"),
            None => Ok(Self::Filesystem { path: s.into() }),
        }
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
        self.stashes.get(alias.as_ref()).cloned()
    }

    pub fn open(&self, pathy: impl AsRef<str>) -> Result<crate::Stash> {
        let name = pathy.as_ref();
        let stash = self.resolve_stash(name).unwrap_or_else(|| Stash {
            key: crate::config::Key::Interactive,
            backend: name.parse().unwrap(),
        });

        stash.try_open(name)
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
key = { source = "macos_keychain", user = "user@example.com"}
backend = { type = "fs", path = "/path/to/stash" }
"#,
        )
        .unwrap();
    }

    #[test]
    fn load_keyfile() {
        use super::Key;
        use std::path::PathBuf;

        let mut path: PathBuf = std::env::var("CARGO_MANIFEST_DIR").unwrap().into();
        assert!(path.pop());
        path.push("keyfile.toml.example");

        let key = Key::KeyFile { path };
        key.get_credentials("stash name").unwrap();
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
        use super::{Backend, Region};

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

        assert_eq!(
            "/example/path".parse::<Backend>().unwrap(),
            Backend::Filesystem {
                path: "/example/path".into()
            }
        )
    }
}
