use super::Result;
use anyhow::Context;
use infinitree_backends::Region;
use serde::{Deserialize, Serialize};
use std::{num::NonZeroUsize, str::FromStr, sync::Arc};

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
            None => Ok(Self::Filesystem { path: s.into() }),
        }
    }
}
