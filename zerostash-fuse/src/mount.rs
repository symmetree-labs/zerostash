//! `mount` subcommand

use std::ffi::OsStr;

use std::io::Cursor;
use std::num::NonZeroUsize;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::{mpsc, Arc};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use infinitree::fields::VersionedMap;
use infinitree::object::AEADReader;
use infinitree::object::Pool;
use infinitree::object::PoolRef;
use infinitree::object::Reader;
use tracing::debug;
use zerostash_files::store::index_buf;

use std::io::Result;

use fuse_mt::*;
use infinitree::Infinitree;
use nix::libc;
use zerostash_files::{restore, Entry, Files};

use crate::chunks::ChunkStack;
use crate::chunks::ChunkStackCache;
use crate::openfile::OpenFile;

pub async fn mount(
    stash: Infinitree<Files>,
    options: &restore::Options,
    mountpoint: &str,
    threads: usize,
) -> anyhow::Result<()> {
    let stash = Arc::new(Mutex::new(stash));
    let (tx, finished) = mpsc::sync_channel(2);
    let destroy_tx = tx.clone();
    ctrlc::set_handler(move || tx.send(()).expect("Could not send signal on channel."))
        .expect("Error setting Ctrl-C handler");

    let stash_clone = Arc::clone(&stash);
    tokio::spawn(async move {
        auto_commit(stash_clone).await;
    });

    let filesystem = ZerostashFS::open(stash, options, destroy_tx, threads).unwrap();
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

async fn auto_commit(stash: Arc<Mutex<Infinitree<Files>>>) {
    let mut interval = tokio::time::interval(Duration::from_secs(180));

    interval.tick().await;
    loop {
        interval.tick().await;

        let mut stash_guard = stash.lock().unwrap();
        let _ = stash_guard.commit("Fuse commit");
        let _ = stash_guard.backend().sync();
        debug!("Committed Changes!");
    }
}

pub struct ZerostashFS {
    pub commit_timestamp: SystemTime,
    pub destroy_tx: mpsc::SyncSender<()>,
    pub stash: Arc<Mutex<Infinitree<Files>>>,
    pub chunks_cache: scc::HashMap<PathBuf, ChunkStackCache>,
    pub threads: usize,
}

impl ZerostashFS {
    pub fn open(
        stash: Arc<Mutex<Infinitree<Files>>>,
        _options: &restore::Options,
        destroy_tx: mpsc::SyncSender<()>,
        threads: usize,
    ) -> Result<Self> {
        stash.lock().unwrap().load_all().unwrap();

        let commit_timestamp = stash
            .lock()
            .unwrap()
            .commit_list()
            .last()
            .unwrap()
            .metadata
            .time;

        Ok(ZerostashFS {
            commit_timestamp,
            destroy_tx,
            stash,
            chunks_cache: scc::HashMap::new(),
            threads,
        })
    }
}

impl FilesystemMT for ZerostashFS {
    fn destroy(&self) {
        debug!("destroy and commit");

        let mut stash = self.stash.lock().unwrap();
        let _ = stash.commit("Fuse commit");
        let _ = stash.backend().sync();
        self.destroy_tx
            .send(())
            .expect("Could not send signal on channel.");
    }

    fn getattr(&self, _req: RequestInfo, path: &Path, _fh: Option<u64>) -> ResultEntry {
        debug!("gettattr = {:?}", path);

        let path_str = path.to_str().unwrap();

        let node = {
            let stash = self.stash.lock().unwrap();
            let index = stash.index();
            let tree = index.directory_tree.read();
            tree.get(path_str)
        };

        if let Some(node) = node {
            if let zerostash_files::Node::Directory(_) = node {
                return Ok((TTL, DIR_ATTR));
            } else {
                let path_string = strip_path(path).to_str().unwrap();
                match self.stash.lock().unwrap().index().files.get(path_string) {
                    Some(metadata) => {
                        let fuse = file_to_fuse(&metadata, self.commit_timestamp);
                        return Ok((TTL, fuse));
                    }
                    None => return Err(libc::ENOENT),
                }
            }
        }

        Err(libc::ENOENT)
    }

    fn opendir(&self, _req: RequestInfo, _path: &Path, _flags: u32) -> ResultOpen {
        debug!("opendir");
        Ok((0, 0))
    }

    fn readdir(&self, _req: RequestInfo, path: &Path, _fh: u64) -> ResultReaddir {
        debug!("readdir: {:?}", path);

        let path_str = path.to_str().unwrap();
        let node = {
            let stash = self.stash.lock().unwrap();
            let index = stash.index();
            let tree = index.directory_tree.read();
            tree.get(path_str).unwrap_or_default()
        };

        let mut vec = vec![];

        match node {
            zerostash_files::Node::Directory(ref dir) => {
                let dir = dir.lock().unwrap();

                for (k, v) in dir.iter() {
                    match v {
                        zerostash_files::Node::File(file) => {
                            let new_entry = DirectoryEntry {
                                name: file.name.clone().into(),
                                kind: fuse_mt::FileType::RegularFile,
                            };
                            vec.push(new_entry);
                        }
                        zerostash_files::Node::Directory(_) => {
                            let new_entry = DirectoryEntry {
                                name: k.clone().into(),
                                kind: fuse_mt::FileType::Directory,
                            };
                            vec.push(new_entry);
                        }
                    }
                }
            }
            zerostash_files::Node::File(_) => return Err(libc::ENOENT),
        }

        Ok(vec)
    }

    fn open(&self, _req: RequestInfo, path: &Path, _flags: u32) -> ResultOpen {
        debug!("open: {:?}", path);
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

        let real_path = strip_path(path);
        let path_string = real_path.to_str().unwrap();
        let entry = {
            let stash = self.stash.lock().unwrap();
            let files = &stash.index().files;
            files.get(path_string).unwrap()
        };
        let file_size = entry.size as usize;
        let offset = offset as usize;

        if offset > file_size {
            return callback(Err(libc::EINVAL));
        }

        let size = size as usize;
        let sort_chunks = || {
            let mut chunks = entry.chunks.clone();
            chunks.sort_by(|(a, _), (b, _)| a.cmp(b));
            chunks
        };
        let mut obj_reader = self.stash.lock().unwrap().storage_reader().unwrap();

        {
            let mut chunks = self
                .chunks_cache
                .entry(real_path.to_path_buf())
                .or_insert_with(|| ChunkStackCache::new(sort_chunks()));
            let chunks = chunks.get_mut();

            if chunks.last_read_offset == offset {
                let end = size.min(file_size - offset);
                if chunks.buf.len() < end {
                    loop {
                        if chunks.read_next(file_size, &mut obj_reader).is_err() {
                            return callback(Err(libc::EINVAL));
                        }

                        if chunks.buf.len() >= end {
                            break;
                        }
                    }
                }
                let ret_buf = chunks.split_buf(end);
                chunks.set_current_read(offset + end);
                return callback(Ok(&ret_buf));
            }
        }

        let mut chunks = ChunkStack::new(sort_chunks(), offset);

        loop {
            if chunks
                .read_next(file_size, offset, &mut obj_reader)
                .is_err()
            {
                return callback(Err(libc::EINVAL));
            }

            if chunks.is_full(size, file_size, offset) {
                let from = chunks.start.unwrap();
                let to = chunks.end.unwrap();
                return callback(Ok(&chunks.buf[from..to]));
            }
        }
    }

    fn write(
        &self,
        _req: RequestInfo,
        path: &Path,
        _fh: u64,
        offset: u64,
        data: Vec<u8>,
        _flags: u32,
    ) -> ResultWrite {
        debug!("write: {:?} {:#x} @ {:#x}", path, data.len(), offset);

        let real_path = strip_path(path);
        let path_string = real_path.to_str().unwrap();

        let entry = {
            let stash = self.stash.lock().unwrap();
            let index = stash.index();
            let files = &index.files;
            files.get(path_string).unwrap()
        };

        let obj_reader = self.stash.lock().unwrap().storage_reader().unwrap();
        let mut buf: Vec<u8> = vec![0; entry.size as usize];
        read_chunks_into_buf(&mut buf, obj_reader, &entry);

        let mut open_file = OpenFile::from_vec(buf);
        let nwritten = match open_file.write_at(offset, data) {
            Ok(nwritten) => nwritten,
            Err(e) => return Err(e),
        };
        let entry_len = open_file.get_len();

        let new_entry = Entry {
            size: entry_len,
            chunks: Vec::new(),
            file_type: entry.file_type.clone(),
            name: entry.name.clone(),
            ..*entry
        };

        let stash = self.stash.lock().unwrap();
        let index = stash.index();
        let hasher = stash.hasher().unwrap();
        let balancer = Pool::new(
            NonZeroUsize::new(self.threads).unwrap(),
            stash.storage_writer().unwrap(),
        )
        .unwrap();

        index_buf(
            open_file.open_file,
            new_entry,
            hasher,
            &index,
            &balancer,
            path_string.to_string(),
        );

        Ok(nwritten)
    }

    fn truncate(&self, _req: RequestInfo, path: &Path, _fh: Option<u64>, size: u64) -> ResultEmpty {
        debug!("truncate {:?}: size {}", path, size);

        let real_path = strip_path(path);
        let path_string = real_path.to_str().unwrap();

        let entry = {
            let stash = self.stash.lock().unwrap();
            let index = stash.index();
            let files = &index.files;
            files.get(path_string).unwrap()
        };

        let obj_reader = self.stash.lock().unwrap().storage_reader().unwrap();
        let mut buf: Vec<u8> = vec![0; entry.size as usize];
        read_chunks_into_buf(&mut buf, obj_reader, &entry);

        buf.truncate(size as usize);
        let len = buf.len() as u64;
        let open_file = Cursor::new(buf);

        let stash = self.stash.lock().unwrap();
        let index = stash.index();
        let hasher = stash.hasher().unwrap();
        let balancer = Pool::new(
            NonZeroUsize::new(self.threads).unwrap(),
            stash.storage_writer().unwrap(),
        )
        .unwrap();

        let new_entry = Entry {
            size: len,
            chunks: Vec::new(),
            file_type: entry.file_type.clone(),
            name: entry.name.clone(),
            ..*entry
        };

        index_buf(
            open_file,
            new_entry,
            hasher,
            &index,
            &balancer,
            path_string.to_string(),
        );

        Ok(())
    }

    fn rename(
        &self,
        _req: RequestInfo,
        parent: &Path,
        name: &OsStr,
        newparent: &Path,
        newname: &OsStr,
    ) -> ResultEmpty {
        debug!(
            "rename: {:?}/{:?} -> {:?}/{:?}",
            parent, name, newparent, newname
        );

        let path = parent.join(name);
        let path_str = strip_path(&path).to_str().unwrap().to_string();
        let new_path = newparent.join(newname);
        let new_path_str = strip_path(&new_path).to_str().unwrap().to_string();

        let is_file = {
            let stash = self.stash.lock().unwrap();
            let index = stash.index();
            let tree = &index.directory_tree;
            let read = tree.read();
            read.is_file(&path_str)
        };

        let stash = self.stash.lock().unwrap();
        let index = stash.index();

        if is_file {
            let files = &index.files;
            rename_file(files, path_str.to_string(), new_path_str.to_string());

            let tree = &index.directory_tree;
            let mut tree = tree.write();

            tree.rename_file(&path_str, newname.to_str().unwrap());
            tree.move_node(&path_str, &new_path_str);
        } else {
            let files = &index.files;
            replace_file_paths(files, &path_str, &new_path_str);

            let tree = &index.directory_tree;
            let mut tree = tree.write();

            tree.move_node(&path_str, &new_path_str);
        }

        Ok(())
    }

    fn mkdir(&self, _req: RequestInfo, parent: &Path, name: &OsStr, _mode: u32) -> ResultEntry {
        debug!("mkdir: {:?}/{:?}", parent, name);

        let path = parent.join(name);
        let stash = self.stash.lock().unwrap();
        let index = stash.index();
        let tree = &index.directory_tree;
        let mut tree = tree.write();
        tree.insert_directory(path.to_str().unwrap(), None);

        Ok((TTL, DIR_ATTR))
    }

    fn rmdir(&self, _req: RequestInfo, parent: &Path, name: &OsStr) -> ResultEmpty {
        debug!("rmdir: {:?}/{:?}", parent, name);

        let path = parent.join(name);
        let path_str = strip_path(&path).to_str().unwrap().to_string();

        let stash = self.stash.lock().unwrap();
        let index = stash.index();

        let tree = &index.directory_tree;
        let mut tree = tree.write();
        tree.remove(&path_str);

        let files = &index.files;
        let mut files_to_delete = vec![];

        files.for_each(|k, _| {
            if k.contains(&path_str) {
                files_to_delete.push(k.clone());
            }
        });

        for path in files_to_delete.iter() {
            files.remove(path.to_string());
        }

        Ok(())
    }

    fn unlink(&self, _req: RequestInfo, parent: &Path, name: &OsStr) -> ResultEmpty {
        debug!("unlink: {:?}/{:?}", parent, name);

        let path = parent.join(name);
        let path_str = strip_path(&path).to_str().unwrap().to_string();

        let stash = self.stash.lock().unwrap();
        let index = stash.index();

        let tree = &index.directory_tree;
        let mut tree = tree.write();
        tree.remove(&path_str);

        let files = &index.files;
        files.remove(path_str);

        Ok(())
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
        self.chunks_cache.remove(real_path);

        Ok(())
    }
}

const TTL: Duration = Duration::from_secs(1);

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

fn read_chunks_into_buf(buf: &mut [u8], mut reader: PoolRef<AEADReader>, entry: &Arc<Entry>) {
    for (start, cp) in entry.chunks.iter() {
        let start = *start as usize;
        reader.read_chunk(cp, &mut buf[start..]).unwrap();
    }
}

fn file_to_fuse(file: &Arc<Entry>, atime: SystemTime) -> FileAttr {
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
        uid: file
            .unix_uid
            .unwrap_or_else(|| nix::unistd::getuid().into()),
        rdev: 0,
        flags: 0,
    }
}

fn strip_path(path: &Path) -> &Path {
    path.strip_prefix("/").unwrap()
}

fn replace_file_paths(files: &VersionedMap<String, Entry>, path_str: &str, new_path_str: &str) {
    let mut new_files = vec![];
    let mut old_paths = vec![];

    files.for_each(|k, v| {
        if k.contains(path_str) {
            let postfix = k.strip_prefix(path_str).unwrap();
            let mut new_file_path = new_path_str.to_string();
            new_file_path.push_str(postfix);
            let new_entry = Entry {
                name: new_file_path.clone(),
                file_type: v.file_type.clone(),
                chunks: v.chunks.clone(),
                ..*v
            };
            new_files.push((new_file_path, new_entry));
            old_paths.push(k.clone());
        }
    });

    for (i, (k, v)) in new_files.iter().enumerate() {
        files.insert(k.to_string(), v.clone());
        files.remove(old_paths[i].clone());
    }
}

fn rename_file(files: &VersionedMap<String, Entry>, path_str: String, new_path_str: String) {
    let entry = files.get(&path_str).unwrap();
    let new_entry = Entry {
        name: new_path_str.clone(),
        file_type: entry.file_type.clone(),
        chunks: entry.chunks.clone(),
        ..*entry
    };

    files.insert(new_path_str, new_entry);
    files.remove(path_str);
}
