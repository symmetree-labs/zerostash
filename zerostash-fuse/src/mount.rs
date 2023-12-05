//! `mount` subcommand

use std::ffi::OsStr;

use std::io::Cursor;
use std::num::NonZeroUsize;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use infinitree::object::AEADReader;
use infinitree::object::Pool;
use infinitree::object::PoolRef;
use infinitree::object::Reader;
use tokio::runtime::Handle;
use tracing::debug;
use zerostash_files::store::index_buf;
use zerostash_files::FileType;
use zerostash_files::Node;

use std::io::Result;

use fuse_mt::*;
use infinitree::Infinitree;
use nix::libc;
use zerostash_files::{Entry, Files};

use crate::chunks::ChunkStack;
use crate::chunks::ChunkStackCache;
use crate::openfile::OpenFile;

pub async fn mount(
    stash: Infinitree<Files>,
    mountpoint: &str,
    threads: usize,
    read_write: bool,
) -> anyhow::Result<()> {
    let stash = Arc::new(stash);

    if read_write {
        stash.load(stash.index().chunks()).unwrap();
        let stash_clone = Arc::clone(&stash);
        tokio::spawn(async move {
            auto_commit(stash_clone).await;
        });
    }

    let mount_type = if read_write { "rw" } else { "ro" };

    let filesystem = ZerostashFS::open(stash, threads, read_write).unwrap();
    let fs = fuse_mt::FuseMT::new(filesystem, 1);

    // Mount the filesystem.
    let handle = spawn_mount(
        fs,
        mountpoint,
        &[
            OsStr::new(mount_type),
            OsStr::new("nodev"),
            OsStr::new("nosuid"),
            OsStr::new("noatime"),
            OsStr::new("fsname=zerostash"),
        ],
    )?;

    // Wait until we are done.
    tokio::signal::ctrl_c().await?;

    // Ensure the filesystem is unmounted.
    handle.join();

    Ok(())
}

async fn auto_commit(stash: Arc<Infinitree<Files>>) {
    let mut interval = tokio::time::interval(Duration::from_secs(180));

    loop {
        interval.tick().await;

        _ = stash.commit("Fuse commit");
        _ = stash.backend().sync();
        debug!("Committed Changes!");
    }
}

pub struct ZerostashFS {
    pub commit_timestamp: SystemTime,
    pub stash: Arc<Infinitree<Files>>,
    pub chunks_cache: scc::HashMap<PathBuf, ChunkStackCache>,
    pub threads: usize,
    pub read_write: bool,
    pub runtime: Handle,
}

impl ZerostashFS {
    pub fn open(stash: Arc<Infinitree<Files>>, threads: usize, read_write: bool) -> Result<Self> {
        stash.load_all().unwrap();

        let commit_timestamp = match stash.commit_list().last() {
            Some(last) => last.metadata.time,
            None => panic!("stash is empty"),
        };

        Ok(ZerostashFS {
            commit_timestamp,
            stash,
            chunks_cache: scc::HashMap::new(),
            threads,
            read_write,
            runtime: Handle::current(),
        })
    }
}

impl FilesystemMT for ZerostashFS {
    fn destroy(&self) {
        debug!("destroy and commit");

        if self.read_write {
            self.runtime.block_on(async {
                _ = self.stash.commit("Fuse commit");
                _ = self.stash.backend().sync();
            });
        }
    }

    fn getattr(&self, _req: RequestInfo, path: &Path, _fh: Option<u64>) -> ResultEntry {
        debug!("gettattr = {:?}", path);

        let path_str = path.to_str().unwrap();

        let node = {
            let index = self.stash.index();
            let tree = &index.tree;
            tree.node_by_path(path_str)
        };

        let Ok(Some(node)) = node else {
            return Err(libc::ENOENT);
        };

        match node.as_ref() {
            Node::File { refs: _, entry } => Ok((TTL, file_to_fuse(entry, self.commit_timestamp))),
            Node::Directory { entries: _ } => Ok((TTL, DIR_ATTR)),
        }
    }

    fn opendir(&self, _req: RequestInfo, _path: &Path, _flags: u32) -> ResultOpen {
        debug!("opendir");
        Ok((0, 0))
    }

    fn readdir(&self, _req: RequestInfo, path: &Path, _fh: u64) -> ResultReaddir {
        debug!("readdir: {:?}", path);

        let path_str = path.to_str().unwrap();
        let node = {
            let index = self.stash.index();
            let tree = &index.tree;
            tree.node_by_path(path_str)
        };

        let Ok(Some(node)) = node else {
            return Err(libc::ENOENT);
        };

        let Node::Directory { entries } = node.as_ref() else {
            return Err(libc::ENOENT);
        };

        let index = self.stash.index();

        let mut vec: Vec<DirectoryEntry> = vec![];

        let mut current = entries.first_entry();
        while let Some(entry) = current {
            if let Some(node) = index.tree.node_by_ref(entry.get()) {
                let kind = if node.is_dir() {
                    fuse_mt::FileType::Directory
                } else {
                    fuse_mt::FileType::RegularFile
                };
                let directory_entry = DirectoryEntry {
                    name: entry.key().clone().into(),
                    kind,
                };
                vec.push(directory_entry);
            }
            current = entry.next();
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
            let index = &self.stash.index();
            let tree = &index.tree;
            let Ok(Some(entry)) = tree.file(path_string) else {
                return callback(Err(libc::EINVAL));
            };
            entry
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
        let mut obj_reader = self.stash.storage_reader().unwrap();

        self.runtime.block_on(async {
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
        })
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
            let index = &self.stash.index();
            let tree = &index.tree;
            let Ok(Some(entry)) = tree.file(path_string) else {
                return Err(libc::EINVAL);
            };
            entry
        };

        let obj_reader = self.stash.storage_reader().unwrap();
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
            ..entry.as_ref().clone()
        };

        let index = self.stash.index();
        let hasher = self.stash.hasher().unwrap();
        let balancer = Pool::new(
            NonZeroUsize::new(self.threads).unwrap(),
            self.stash.storage_writer().unwrap(),
        )
        .unwrap();

        index_buf(
            open_file.open_file.into_inner(),
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
            let index = &self.stash.index();
            let tree = &index.tree;
            let Ok(Some(entry)) = tree.file(path_string) else {
                return Err(libc::EINVAL);
            };
            entry
        };

        let obj_reader = self.stash.storage_reader().unwrap();
        let mut buf: Vec<u8> = vec![0; entry.size as usize];
        read_chunks_into_buf(&mut buf, obj_reader, &entry);

        buf.truncate(size as usize);
        let len = buf.len() as u64;
        let open_file = Cursor::new(buf);

        let index = self.stash.index();
        let hasher = self.stash.hasher().unwrap();
        let balancer = Pool::new(
            NonZeroUsize::new(self.threads).unwrap(),
            self.stash.storage_writer().unwrap(),
        )
        .unwrap();

        let new_entry = Entry {
            size: len,
            chunks: Vec::new(),
            file_type: entry.file_type.clone(),
            name: entry.name.clone(),
            ..entry.as_ref().clone()
        };

        index_buf(
            open_file.into_inner(),
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
        let index = self.stash.index();
        let tree = &index.tree;

        if tree.move_node(&path_str, &new_path_str).is_err() {
            return Err(libc::EIO);
        }

        Ok(())
    }

    fn mkdir(&self, _req: RequestInfo, parent: &Path, name: &OsStr, _mode: u32) -> ResultEntry {
        debug!("mkdir: {:?}/{:?}", parent, name);

        let path = parent.join(name);
        let index = self.stash.index();
        let tree = &index.tree;

        if tree.insert_directory(path.to_str().unwrap()).is_err() {
            return Err(libc::EIO);
        }

        Ok((TTL, DIR_ATTR))
    }

    fn rmdir(&self, _req: RequestInfo, parent: &Path, name: &OsStr) -> ResultEmpty {
        debug!("rmdir: {:?}/{:?}", parent, name);

        let path = parent.join(name);
        let path_str = strip_path(&path).to_str().unwrap().to_string();

        let index = self.stash.index();
        let tree = &index.tree;

        if tree.remove(&path_str).is_err() {
            return Err(libc::EIO);
        }

        Ok(())
    }

    fn unlink(&self, _req: RequestInfo, parent: &Path, name: &OsStr) -> ResultEmpty {
        debug!("unlink: {:?}/{:?}", parent, name);

        let path = parent.join(name);
        let path_str = strip_path(&path).to_str().unwrap().to_string();

        let index = self.stash.index();
        let tree = &index.tree;

        if tree.remove(&path_str).is_err() {
            return Err(libc::EIO);
        }

        Ok(())
    }

    fn create(
        &self,
        _req: RequestInfo,
        parent: &Path,
        name: &OsStr,
        mode: u32,
        flags: u32,
    ) -> ResultCreate {
        debug!("create {:?}/{:?}", parent, name);
        let real_path = parent.join(name);
        let path_string = strip_path(&real_path).to_str().unwrap();

        let now = SystemTime::now();
        let unix = now.duration_since(UNIX_EPOCH).unwrap();
        let name = name.to_str().unwrap().to_string();

        let entry = Arc::new(Entry {
            unix_secs: unix.as_secs() as i64,
            unix_nanos: unix.as_nanos() as u32,
            unix_perm: Some(mode),
            unix_uid: Some(nix::unistd::getuid().into()),
            unix_gid: Some(nix::unistd::getgid().into()),
            readonly: None,
            file_type: FileType::File,
            size: 0,
            name,
            chunks: Vec::new(),
        });

        let attr = file_to_fuse(&entry, SystemTime::now());

        let index = self.stash.index();
        let tree = &index.tree;
        if tree
            .insert_file(path_string, entry.as_ref().clone())
            .is_err()
        {
            return Err(libc::EIO);
        }

        Ok(CreatedEntry {
            ttl: TTL,
            attr,
            fh: 0,
            flags,
        })
    }

    fn chmod(&self, _req: RequestInfo, path: &Path, _fh: Option<u64>, mode: u32) -> ResultEmpty {
        debug!("chmod: {:?} {:#o}", path, mode);
        let path_string = strip_path(path).to_str().unwrap().to_string();

        let mut index = self.stash.index().clone();

        let tree = &mut index.tree;
        let Ok(Some(entry)) = tree.file(&path_string) else {
            return Err(libc::EINVAL);
        };

        let new_entry = Entry {
            unix_perm: Some(mode),
            chunks: entry.chunks.clone(),
            file_type: entry.file_type.clone(),
            name: entry.name.clone(),
            ..entry.as_ref().clone()
        };

        if tree.update_file(&path_string, new_entry).is_err() {
            return Err(libc::ENOENT);
        }

        Ok(())
    }

    fn chown(
        &self,
        _req: RequestInfo,
        path: &Path,
        _fh: Option<u64>,
        uid: Option<u32>,
        gid: Option<u32>,
    ) -> ResultEmpty {
        debug!("chown {:?} to {:?}:{:?}", path, uid, gid);
        let path_string = strip_path(path).to_str().unwrap().to_string();

        let index = self.stash.index();
        let tree = &index.tree;
        let Ok(Some(entry)) = tree.file(&path_string) else {
            return Err(libc::EINVAL);
        };

        let new_entry = Entry {
            unix_uid: Some(uid.unwrap_or_else(|| {
                entry
                    .unix_uid
                    .unwrap_or_else(|| nix::unistd::getuid().into())
            })),
            unix_gid: Some(gid.unwrap_or_else(|| {
                entry
                    .unix_gid
                    .unwrap_or_else(|| nix::unistd::getgid().into())
            })),
            chunks: entry.chunks.clone(),
            file_type: entry.file_type.clone(),
            name: entry.name.clone(),
            ..entry.as_ref().clone()
        };

        if tree.update_file(&path_string, new_entry).is_err() {
            return Err(libc::ENOENT);
        }

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
    kind: fuse_mt::FileType::Directory,
    perm: 0o777,
    nlink: 1,
    uid: 1000,
    gid: 1000,
    rdev: 0,
    flags: 0,
};

fn read_chunks_into_buf(buf: &mut [u8], mut reader: PoolRef<AEADReader>, entry: &Entry) {
    for (start, cp) in entry.chunks.iter() {
        let start = *start as usize;
        reader.read_chunk(cp, &mut buf[start..]).unwrap();
    }
}

fn file_to_fuse(file: &Entry, atime: SystemTime) -> FileAttr {
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
        kind: match_filetype(file.file_type.clone()),
        perm: (file.unix_perm.unwrap() & 0o777) as u16,
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

fn match_filetype(file_type: FileType) -> fuse_mt::FileType {
    match file_type {
        FileType::File => fuse_mt::FileType::RegularFile,
        FileType::Symlink(_) => fuse_mt::FileType::Symlink,
        FileType::Directory => panic!("Must be a file!"),
    }
}
