use infinitree::ChunkPointer;

use std::{
    fs,
    path::{Component, Path, PathBuf},
    sync::Arc,
    time::{SystemTimeError, UNIX_EPOCH},
};

macro_rules! if_yes {
    ( $flag:expr, $val:expr ) => {
        if $flag {
            Some($val)
        } else {
            None
        }
    };
}

#[derive(thiserror::Error, Debug)]
pub enum EntryError {
    #[error("Path contains `..` or `.` in a non-prefix position")]
    InvalidInputPath,
    #[error("Time error: {source}")]
    Time {
        #[from]
        source: SystemTimeError,
    },
    #[error("IO error: {source}")]
    IO {
        #[from]
        source: std::io::Error,
    },
}

pub(crate) fn normalize_filename(path: &impl AsRef<Path>) -> Result<String, EntryError> {
    let path = path.as_ref();

    Ok(path
        .components()
        .map(|c| match c {
            Component::Normal(val) => Ok(val.to_string_lossy()),
            _ => Err(EntryError::InvalidInputPath),
        })
        // skip leading components that are invalid
        .skip_while(Result::is_err)
        .collect::<Result<Vec<_>, _>>()?
        .join("/"))
}

#[derive(Clone, Serialize, Deserialize, Default)]
pub struct Entry {
    pub unix_secs: u64,
    pub unix_nanos: u32,
    pub unix_perm: Option<u32>,
    pub unix_uid: Option<u32>,
    pub unix_gid: Option<u32>,
    pub readonly: Option<bool>,
    pub symlink: Option<PathBuf>,

    pub size: u64,
    pub name: String,

    pub chunks: Vec<(u64, Arc<ChunkPointer>)>,
}

impl PartialEq for Entry {
    fn eq(&self, other: &Self) -> bool {
        // ignore chunks in comparison, as they may not be available
        self.unix_gid == other.unix_gid
            && self.unix_uid == other.unix_uid
            && self.unix_secs == other.unix_secs
            && self.unix_nanos == other.unix_nanos
            && self.unix_perm == other.unix_perm
            && self.size == other.size
            && self.readonly == other.readonly
            && self.name == other.name
            && self.symlink == other.symlink
    }
}

impl Entry {
    #[cfg(windows)]
    pub fn from_metadata(
        metadata: fs::Metadata,
        path: &impl AsRef<Path>,
        preserve_permissions: bool,
        preserve_ownership: bool,
    ) -> Result<Entry, EntryError> {
        let path = path.as_ref();
        let (unix_secs, unix_nanos) = to_unix_mtime(&metadata)?;

        Ok(Entry {
            unix_secs,
            unix_nanos,
            unix_perm: 0,
            unix_uid: None,
            unix_gid: None,
            symlink: if metadata.is_symlink() {
                fs::read_link(path).ok()
            } else {
                None
            },

            readonly: if_yes!(preserve_permissions, metadata.permissions().readonly()),

            size: metadata.len(),
            name: normalize_filename(path)?,

            chunks: Vec::new(),
        })
    }

    #[cfg(unix)]
    pub fn from_metadata(
        metadata: fs::Metadata,
        path: &impl AsRef<Path>,
        preserve_permissions: bool,
        preserve_ownership: bool,
    ) -> Result<Entry, EntryError> {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};

        let perms = metadata.permissions();
        let (unix_secs, unix_nanos) = to_unix_mtime(&metadata)?;

        debug_assert_eq!(unix_secs, metadata.mtime() as u64);
        debug_assert_eq!(unix_nanos as i64, metadata.mtime_nsec());

        Ok(Entry {
            unix_secs,
            unix_nanos,

            unix_perm: if_yes!(preserve_permissions, perms.mode()),
            unix_uid: if_yes!(preserve_ownership, metadata.uid()),
            unix_gid: if_yes!(preserve_ownership, metadata.gid()),
            readonly: if_yes!(preserve_permissions, metadata.permissions().readonly()),
            symlink: if metadata.is_symlink() {
                fs::read_link(path).ok()
            } else {
                None
            },

            size: metadata.len(),
            name: normalize_filename(&path)?,

            chunks: Vec::new(),
        })
    }

    #[cfg(unix)]
    pub fn restore_to(&self, file: &fs::File) -> Result<(), EntryError> {
        use std::{
            os::unix::{fs::PermissionsExt, prelude::AsRawFd},
            time::{Duration, SystemTime},
        };

        file.set_len(self.size)?;

        if let Some(perm) = self.unix_perm {
            file.set_permissions(fs::Permissions::from_mode(perm))?;
        }

        let atime = SystemTime::now().duration_since(UNIX_EPOCH)?.into();
        let mtime = Duration::new(self.unix_secs, self.unix_nanos).into();
        nix::sys::stat::futimens(file.as_raw_fd(), &atime, &mtime).unwrap();

        Ok(())
    }
}

fn to_unix_mtime(m: &fs::Metadata) -> Result<(u64, u32), EntryError> {
    let mtime = m.modified()?.duration_since(UNIX_EPOCH)?;
    Ok((mtime.as_secs(), mtime.subsec_nanos()))
}
