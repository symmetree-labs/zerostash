use infinitree::{index, ChunkPointer};

use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::UNIX_EPOCH,
};

pub type FileSet = index::Map<PathBuf, Arc<Entry>>;

#[derive(Hash, Clone, Eq, PartialEq, Serialize, Deserialize, Default)]
pub struct Entry {
    pub unix_secs: u64,
    pub unix_nanos: u32,
    pub unix_perm: u32,
    pub unix_uid: u32,
    pub unix_gid: u32,

    pub size: u64,
    pub readonly: bool,
    pub name: String,

    pub chunks: Vec<(u64, ChunkPointer)>,
}

impl Entry {
    #[cfg(windows)]
    pub fn from_metadata(
        metadata: fs::Metadata,
        path: impl AsRef<Path>,
    ) -> Result<Entry, Box<dyn Error>> {
        let path = path.as_ref();
        let (unix_secs, unix_nanos) = to_unix_mtime(&metadata)?;

        Ok(File {
            unix_secs,
            unix_nanos,
            unix_perm: 0,
            unix_uid: 0,
            unix_gid: 0,

            size: metadata.len(),
            readonly: metadata.permissions().readonly(),
            name: path.as_ref().to_str().unwrap().to_string(),

            chunks: Vec::new(),
        })
    }

    #[cfg(unix)]
    pub fn from_metadata(
        metadata: fs::Metadata,
        path: impl AsRef<Path>,
    ) -> Result<Entry, Box<dyn Error>> {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};

        let perms = metadata.permissions();
        let (unix_secs, unix_nanos) = to_unix_mtime(&metadata)?;

        Ok(Entry {
            unix_secs,
            unix_nanos,
            unix_perm: perms.mode(),
            unix_uid: metadata.uid(),
            unix_gid: metadata.gid(),

            size: metadata.len(),
            readonly: metadata.permissions().readonly(),
            name: path.as_ref().to_str().unwrap().to_string(),

            chunks: Vec::new(),
        })
    }
}

fn to_unix_mtime(m: &fs::Metadata) -> Result<(u64, u32), Box<dyn Error>> {
    let mtime = m.modified()?.duration_since(UNIX_EPOCH)?;
    Ok((mtime.as_secs(), mtime.subsec_nanos()))
}
