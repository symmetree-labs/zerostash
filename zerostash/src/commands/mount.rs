//! `mount` subcommand

use std::ffi::OsStr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
use std::{collections::HashMap, path::Path};

use std::io::Result;

use crate::prelude::*;
use fuse_mt::*;
use infinitree::object::Reader;
use infinitree::Infinitree;
use nix::libc;
use zerostash_files::{restore, Entry, Files};

#[derive(Command, Debug)]
pub struct Mount {
    #[clap(flatten)]
    stash: StashArgs,

    #[clap(flatten)]
    options: restore::Options,

    /// The location the filesytem will be mounted on
    #[clap(short = 'T', long = "target")]
    mount_point: String,
}

#[cfg(unix)]
#[async_trait]
impl AsyncRunnable for Mount {
    /// Start the application.
    async fn run(&self) {
        let stash = self.stash.open();

        if let Err(e) = mount(stash, &self.options, &self.mount_point) {
            panic!("Error = {}", e)
        }
    }
}

fn mount(
    stash: Infinitree<Files>,
    options: &restore::Options,
    mountpoint: &str,
) -> anyhow::Result<()> {
    let filesystem = SimpleFs::open(stash, options).unwrap();
    let fuse_args = [OsStr::new("-o"), OsStr::new("fsname=zerostash")];

    let fs = fuse_mt::FuseMT::new(filesystem, 1);

    fuse_mt::mount(fs, mountpoint, &fuse_args[..])?;

    Ok(())
}

type DirPath = PathBuf;
type FilePath = PathBuf;
type FileContent = Vec<u8>;
type Metadata = Arc<Entry>;

struct SimpleFs {
    stash: Infinitree<Files>,
    file_map: HashMap<FilePath, Metadata>,
    file_content: Mutex<HashMap<FilePath, FileContent>>,
    dir_map: HashMap<DirPath, Vec<DirectoryEntry>>,
    file_attr_memory: Mutex<HashMap<PathBuf, FileAttr>>,
}

fn add_dir_to_map(
    dir_map: &mut HashMap<DirPath, Vec<DirectoryEntry>>,
    path: &Path,
    kind: FileType,
) {
    let name = path
        .file_name()
        .expect("All files have filenames")
        .to_owned();

    let parent = path
        .parent()
        .expect("Paths should have parents")
        .to_path_buf();

    if !dir_map.contains_key(&parent) {
        add_dir_to_map(dir_map, &parent, FileType::Directory);
    }

    dir_map
        .entry(parent)
        .or_default()
        .push(DirectoryEntry { name, kind });
}

const DIR_ATTR: FileAttr = FileAttr {
    size: 0,
    blocks: 0,
    atime: SystemTime::UNIX_EPOCH,
    mtime: SystemTime::UNIX_EPOCH,
    ctime: SystemTime::UNIX_EPOCH,
    crtime: SystemTime::UNIX_EPOCH,
    kind: FileType::Directory,
    perm: 0o444,
    nlink: 1,
    uid: 1000,
    gid: 1000,
    rdev: 0,
    flags: 0,
};

pub fn match_filetype(file_type: &zerostash_files::FileType) -> FileType {
    match file_type {
        zerostash_files::FileType::File => FileType::RegularFile,
        zerostash_files::FileType::Symlink(_) => FileType::Symlink,
        zerostash_files::FileType::Directory => panic!("Didnt expect a directory!"),
    }
}

impl SimpleFs {
    pub fn open(stash: Infinitree<Files>, options: &restore::Options) -> Result<Self> {
        let mut dir_map: HashMap<DirPath, Vec<DirectoryEntry>> = HashMap::new();
        dir_map.insert(PathBuf::new(), vec![]);
        let mut file_map = HashMap::new();

        for file in options.list(&stash) {
            let file_name_path = PathBuf::from(&file.name);
            let filetype = match_filetype(&file.file_type);

            add_dir_to_map(&mut dir_map, &file_name_path, filetype);
            file_map.insert(file_name_path, file.clone());
        }

        Ok(SimpleFs {
            stash,
            file_map,
            file_content: Mutex::new(HashMap::new()),
            dir_map,
            file_attr_memory: Mutex::new(HashMap::new()),
        })
    }
}

const TTL: Duration = Duration::from_secs(1);

pub fn file_to_fuse(file: &Arc<Entry>) -> FileAttr {
    FileAttr {
        size: file.size,
        blocks: 1,
        atime: SystemTime::now(),
        mtime: SystemTime::now(),
        ctime: SystemTime::now(),
        crtime: SystemTime::UNIX_EPOCH,
        kind: FileType::RegularFile,
        perm: 0o444,
        nlink: 1,
        gid: file.unix_gid.unwrap_or(0),
        uid: file.unix_uid.unwrap_or(0),
        rdev: 0,
        flags: 0,
    }
}

fn strip_path(path: &Path) -> &Path {
    path.strip_prefix("/").unwrap()
}

impl FilesystemMT for SimpleFs {
    fn getattr(&self, _req: RequestInfo, path: &Path, _fh: Option<u64>) -> ResultEntry {
        println!("getattr = {:?}", path);

        let real_path = strip_path(path);
        let mut file_attr_memory = self.file_attr_memory.lock().unwrap();

        if self.dir_map.contains_key(real_path) {
            Ok((TTL, DIR_ATTR))
        } else if let Some(file_attr) = file_attr_memory.get(real_path) {
            Ok((TTL, *file_attr))
        } else {
            match self.file_map.get(real_path) {
                Some(metadata) => {
                    let fuse = file_to_fuse(metadata);
                    file_attr_memory.insert(real_path.into(), fuse);
                    Ok((TTL, fuse))
                }
                None => Err(libc::ENOENT),
            }
        }
    }

    fn opendir(&self, _req: RequestInfo, _path: &Path, _flags: u32) -> ResultOpen {
        println!("(opendir: {:?} flags = {:#o})", _path, _flags);
        Ok((0, 0))
    }

    fn readdir(&self, _req: RequestInfo, path: &Path, _fh: u64) -> ResultReaddir {
        println!("readdir: {:?}", path);
        Ok(self
            .dir_map
            .get(strip_path(path))
            .cloned()
            .unwrap_or_default())
    }
    fn open(&self, _req: RequestInfo, path: &Path, _flags: u32) -> ResultOpen {
        println!("open: {:?} (flags = {:#x})", path, _flags);

        let mut file_content = self.file_content.lock().unwrap();
        let real_path = strip_path(path);

        if file_content.get(real_path).is_none() {
            let metadata = self.file_map.get(real_path).unwrap();
            let mut objectreader = self.stash.storage_reader().unwrap();
            let mut buf = vec![];
            buf.resize(metadata.size as usize, 0);

            for (start, cp) in metadata.chunks.iter() {
                let start = *start as usize;
                objectreader.read_chunk(cp, &mut buf[start..]).unwrap();
            }

            file_content.insert(PathBuf::from(real_path), buf);
        }

        Ok((0, 0))
    }
    fn read(
        &self,
        _req: RequestInfo,
        path: &Path,
        _fh: u64,
        offset: u64,
        size: u32,
        callback: impl FnOnce(ResultSlice<'_>) -> CallbackResult,
    ) -> CallbackResult {
        println!("read: {:?} {:#x} @ {:#x}", path, size, offset);

        let file_content = self.file_content.lock().unwrap();
        let real_path = strip_path(path);
        let metadata = self.file_map.get(real_path).unwrap();

        if offset > metadata.size {
            return callback(Err(libc::EINVAL));
        }

        let buf: Vec<u8> = match file_content.get(real_path) {
            Some(buf) => buf.to_vec(),
            None => return callback(Err(libc::ENOENT)),
        };

        let end = usize::min(offset as usize + size as usize, metadata.size as usize);
        let buf: Vec<u8> = buf[offset as usize..end].to_vec();

        callback(Ok(&buf))
    }
}
