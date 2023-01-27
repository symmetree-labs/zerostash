use super::Result;
use anyhow::Context;
use infinitree_backends::Region;
use serde::{Deserialize, Serialize};
use std::{
    num::NonZeroUsize,
    path::{Component, Path, PathBuf},
    str::FromStr,
    sync::Arc,
};

/// Backend configuration
/// This may be specific to the backend type
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[serde(tag = "type")]
#[non_exhaustive]
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
    pub(super) fn to_infinitree(&self) -> Result<Arc<dyn infinitree::backends::Backend>> {
        use Backend::*;

        let backend: Arc<dyn infinitree::backends::Backend> = match self {
            Filesystem { path } => infinitree::backends::Directory::new(path)?,
            S3 {
                bucket,
                region,
                keys,
            } => {
                use infinitree_backends::{Credentials, S3};

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
            } => infinitree_backends::Cache::new(
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
            None => {
                let path = match std::fs::canonicalize(s) {
                    Ok(s) => s,
                    Err(_) => normalize_path(Path::new(s)),
                }
                .to_string_lossy()
                .to_string();

                Ok(Self::Filesystem { path })
            }
        }
    }
}

// originally lifted from
// https://github.com/rust-lang/cargo/blob/fede83ccf973457de319ba6fa0e36ead454d2e20/src/cargo/util/paths.rs#L61
pub fn normalize_path(path: &Path) -> PathBuf {
    let current_dir = std::env::current_dir().unwrap();
    let mut components = path.components().peekable();

    let mut ret = match components.peek().cloned() {
        Some(c @ Component::Prefix(..)) => {
            components.next();
            PathBuf::from(c.as_os_str())
        }
        Some(c @ Component::RootDir) => {
            components.next();
            c.as_os_str().into()
        }
        _ => current_dir,
    };

    for component in components {
        match component {
            Component::Prefix(..) => unreachable!(),
            Component::RootDir => {
                ret.push(component.as_os_str());
            }
            Component::CurDir => {}
            Component::ParentDir => {
                ret.pop();
            }
            Component::Normal(c) => {
                ret.push(c);
            }
        }
    }
    ret
}

#[cfg(test)]
mod test {
    use std::path::PathBuf;

    // Ignore this test on Windows because file path prefixes make
    // checking equality something i can't debug on a CI.
    #[test]
    #[cfg(not(target_os = "windows"))]
    fn normalize_path() {
        use super::normalize_path;
        use std::fs::canonicalize;
        use std::path::Path;

        let current_dir = std::env::current_dir()
            .unwrap()
            .to_string_lossy()
            .to_string();

        assert_eq!(
            normalize_path(Path::new("../zerostash")),
            canonicalize(format!("{current_dir}/../zerostash")).unwrap()
        );

        assert_eq!(
            normalize_path(Path::new("./src")),
            canonicalize(format!("{current_dir}/src")).unwrap()
        );

        assert_eq!(
            normalize_path(Path::new("src")),
            canonicalize(format!("{current_dir}/src")).unwrap()
        );

        assert_eq!(
            normalize_path(Path::new("/zerostash")),
            PathBuf::from("/zerostash")
        );
    }
}
