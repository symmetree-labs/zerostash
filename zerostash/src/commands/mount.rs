//! `mount` subcommand

use std::cell::RefCell;
use std::ffi::OsStr;
use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use std::vec;
use std::{collections::HashMap, path::Path};
use tracing::debug;

use std::io::Result;

use crate::prelude::*;
use fuse_mt::*;
use infinitree::object::Reader;
use infinitree::{ChunkPointer, Infinitree};
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

pub fn mount(
    stash: Infinitree<Files>,
    options: &restore::Options,
    mountpoint: &str,
) -> anyhow::Result<()> {
    let (tx, finished) = mpsc::sync_channel(2);
    let destroy_tx = tx.clone();
    ctrlc::set_handler(move || tx.send(()).expect("Could not send signal on channel."))
        .expect("Error setting Ctrl-C handler");

    let filesystem = ZerostashFS::open(stash, options, destroy_tx).unwrap();
    let fuse_args = [OsStr::new("-o"), OsStr::new("fsname=zerostash")];

    let fs = fuse_mt::FuseMT::new(filesystem, 1);

    // Mount the filesystem.
    let handle = spawn_mount(fs, mountpoint, &fuse_args[..])?;

    // Wait until we are done.
    finished.recv().expect("Could not receive from channel.");

    // Ensure the filesystem is unmounted.
    handle.join();

    Ok(())
}

type ChunkData = (Vec<u8>, usize, usize);

struct ZerostashFS {
    commit_timestamp: SystemTime,
    destroy_tx: mpsc::SyncSender<()>,
    stash: Infinitree<Files>,
    file_map: HashMap<PathBuf, Arc<Entry>>,
    dir_map: HashMap<PathBuf, Vec<DirectoryEntry>>,
    stack: Mutex<HashMap<PathBuf, RefCell<ChunkData>>>,
}

fn add_dir_to_map(
    dir_map: &mut HashMap<PathBuf, Vec<DirectoryEntry>>,
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

impl ZerostashFS {
    pub fn open(
        stash: Infinitree<Files>,
        options: &restore::Options,
        destroy_tx: mpsc::SyncSender<()>,
    ) -> Result<Self> {
        let mut dir_map = HashMap::new();
        dir_map.insert(PathBuf::new(), vec![]);
        let mut file_map = HashMap::new();

        for file in options.list(&stash) {
            let file_name_path = PathBuf::from(&file.name);
            let filetype = match_filetype(&file.file_type);

            add_dir_to_map(&mut dir_map, &file_name_path, filetype);
            file_map.insert(file_name_path, file.clone());
        }
        let commit_timestamp = stash.commit_list().last().unwrap().metadata.time;

        Ok(ZerostashFS {
            commit_timestamp,
            destroy_tx,
            stash,
            file_map,
            dir_map,
            stack: Mutex::new(HashMap::new()),
        })
    }
}

const TTL: Duration = Duration::from_secs(1);

pub fn file_to_fuse(file: &Arc<Entry>, atime: SystemTime) -> FileAttr {
    let mtime = UNIX_EPOCH
        + Duration::from_secs(file.unix_secs as u64)
        + Duration::from_nanos(file.unix_nanos as u64);
    FileAttr {
        size: file.size,
        blocks: 1,
        atime,
        mtime,
        ctime: mtime,
        crtime: SystemTime::UNIX_EPOCH,
        kind: FileType::RegularFile,
        perm: 0o444,
        nlink: 1,
        gid: file
            .unix_gid
            .unwrap_or_else(|| nix::unistd::getgid().into()),
        uid: file.unix_uid.unwrap_or(0),
        rdev: 0,
        flags: 0,
    }
}

fn strip_path(path: &Path) -> &Path {
    path.strip_prefix("/").unwrap()
}

impl FilesystemMT for ZerostashFS {
    fn destroy(&self) {
        debug!("destroy");
        self.destroy_tx
            .send(())
            .expect("Could not send signal on channel.")
    }
    fn getattr(&self, _req: RequestInfo, path: &Path, _fh: Option<u64>) -> ResultEntry {
        debug!("gettattr = {:?}", path);

        let real_path = strip_path(path);

        if self.dir_map.contains_key(real_path) {
            Ok((TTL, DIR_ATTR))
        } else {
            match self.file_map.get(real_path) {
                Some(metadata) => {
                    let fuse = file_to_fuse(metadata, self.commit_timestamp);
                    Ok((TTL, fuse))
                }
                None => Err(libc::ENOENT),
            }
        }
    }

    fn opendir(&self, _req: RequestInfo, _path: &Path, _flags: u32) -> ResultOpen {
        debug!("opendir");
        Ok((0, 0))
    }

    fn readdir(&self, _req: RequestInfo, path: &Path, _fh: u64) -> ResultReaddir {
        debug!("readdir: {:?}", path);
        Ok(self
            .dir_map
            .get(strip_path(path))
            .cloned()
            .unwrap_or_default())
    }
    fn open(&self, _req: RequestInfo, path: &Path, _flags: u32) -> ResultOpen {
        debug!("open: {:?}", path);
        let real_path = strip_path(path);
        let mut stack = self.stack.lock().unwrap();
        stack.insert(real_path.to_path_buf(), RefCell::new((vec![], 0, 0)));
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
        debug!("read: {:?} {:#x} @ {:#x}", path, size, offset);
        let size = size as usize;
        let offset = offset as usize;
        let real_path = strip_path(path);
        let metadata = self.file_map.get(real_path).unwrap();
        let metadata_size = metadata.size as usize;
        let mut chunks = metadata.chunks.clone();
        chunks.sort_by(|(a, _), (b, _)| a.cmp(b));
        let mut stack = self.stack.lock().unwrap();
        let stack = stack
            .entry(real_path.to_path_buf())
            .or_insert(RefCell::new((vec![], 0, 0)));
        let mut stack = stack.borrow_mut();
        let mut objectreader = self.stash.storage_reader().unwrap();

        if offset > metadata_size {
            return callback(Err(libc::EINVAL));
        }

        let current_read = stack.2;
        if current_read == offset {
            let end = usize::min(size, metadata_size - offset);
            if stack.0.len() < end {
                loop {
                    let (start, chunk_p) = chunks.get(stack.1).unwrap();
                    let arc = (metadata_size as u64, Arc::new(ChunkPointer::default()));
                    let (next_start, _) = chunks.get(stack.1 + 1).unwrap_or(&arc);
                    let buf_size = next_start - start;
                    let mut buf = vec![0; buf_size as usize];
                    objectreader.read_chunk(chunk_p, &mut buf).unwrap();
                    stack.0.append(&mut buf);
                    stack.1 = stack.1.wrapping_add(1);
                    if stack.0.len() >= usize::min(size, metadata_size - offset) {
                        break;
                    }
                }
            }
            let ret_buf = stack.0[..end].to_vec();
            stack.0 = stack.0[end..].to_vec();
            stack.2 = offset + end;
            return callback(Ok(&ret_buf));
        }

        for (i, (chunk_offset, _)) in chunks.iter().enumerate() {
            let chunk_offset = *chunk_offset as usize;
            if offset < chunk_offset {
                let mut chunk_index = usize::max(i - 1, 0); //0
                let mut buf = vec![];
                let mut from;
                let to;
                let (start_offset, _) = chunks.get(chunk_index).unwrap();
                loop {
                    let (start, chunk_p) = chunks.get(chunk_index).unwrap();
                    let arc = (metadata_size as u64, Arc::new(ChunkPointer::default()));
                    let (next_start, _) = chunks.get(chunk_index + 1).unwrap_or(&arc);
                    chunk_index += 1;
                    let buf_size = next_start - start;
                    let mut temp_buf = vec![0; buf_size as usize];
                    objectreader.read_chunk(chunk_p, &mut temp_buf).unwrap();
                    buf.append(&mut temp_buf);
                    from = offset - *start_offset as usize;
                    if buf[from..].len() > size {
                        from = offset - *start_offset as usize;
                        to = from + size;
                        break;
                    }
                }
                return callback(Ok(&buf[from..to]));
            }
        }

        let (c_offset, pointer) = chunks.last().unwrap();
        let c_offset = *c_offset as usize;
        let buf_size = metadata_size - c_offset;
        let mut buf = vec![0; buf_size];
        objectreader.read_chunk(pointer, &mut buf).unwrap();
        let from = offset - c_offset;
        let to = usize::min(from + size, buf.len());
        callback(Ok(&buf[from..to]))
    }

    fn release(
        &self,
        _req: RequestInfo,
        path: &Path,
        _fh: u64,
        _flags: u32,
        _lock_owner: u64,
        _flush: bool,
    ) -> ResultEmpty {
        debug!("release {:?}", path);
        let real_path = strip_path(path);
        let mut stack = self.stack.lock().unwrap();

        stack.remove(real_path);

        Ok(())
    }
}
