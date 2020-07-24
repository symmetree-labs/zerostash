use crate::{
    backends::Backend,
    chunks, files,
    meta::{self, ReadError},
    objects,
};
pub use crate::{crypto::StashKey, meta::ObjectIndex};

use std::path::Path;
use std::sync::Arc;

pub(crate) mod restore;
pub(crate) mod store;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub struct Stash {
    backend: Arc<dyn Backend>,
    chunks: chunks::ChunkStore,
    files: files::FileStore,
    master_key: StashKey,
}

impl Stash {
    pub fn new(backend: Arc<dyn Backend>, master_key: StashKey) -> Stash {
        let chunks = chunks::ChunkStore::default();
        let files = files::FileStore::default();

        Stash {
            backend,
            chunks,
            files,
            master_key,
        }
    }

    pub fn read(&mut self) -> Result<&Self> {
        let mut metareader =
            meta::Reader::new(self.backend.clone(), self.master_key.get_meta_crypto()?);
        let mut next_object = Some(self.master_key.root_object_id()?);

        while let Some(header) = match next_object {
            Some(ref o) => Some(metareader.open(o)?),
            None => None,
        } {
            next_object = header.next_object();

            match metareader.read_into(&mut self.files) {
                Ok(_) | Err(ReadError::NoField) => (),
                Err(e) => return Err(e.into()),
            };

            match metareader.read_into(&mut self.files) {
                Ok(_) | Err(ReadError::NoField) => (),
                Err(e) => return Err(e.into()),
            };
        }

        Ok(self)
    }

    pub fn list<'a>(&'a self, glob: &'a [impl AsRef<str>]) -> restore::FileIterator<'a> {
        let matchers = glob
            .iter()
            .map(|g| glob::Pattern::new(g.as_ref()).unwrap())
            .collect::<Vec<glob::Pattern>>();
        let base_iter = self.file_index().into_iter().map(|r| r.key().clone());

        match glob.len() {
            i if i == 0 => Box::new(base_iter),
            _ => Box::new(base_iter.filter(move |f| matchers.iter().any(|m| m.matches(&f.name)))),
        }
    }

    pub fn restore_by_glob(
        &mut self,
        threads: usize,
        pattern: &[impl AsRef<str>],
        target: impl AsRef<Path>,
    ) -> Result<()> {
        restore::from_iter(
            threads,
            self.list(pattern),
            self.backend.clone(),
            self.master_key.get_object_crypto()?,
            target,
        );

        Ok(())
    }

    pub fn add_recursive(&mut self, threads: usize, path: impl AsRef<Path>) -> Result<()> {
        let mut objstore =
            objects::Storage::new(self.backend.clone(), self.master_key.get_object_crypto()?);

        store::recursive(
            threads,
            &mut self.chunks,
            &mut self.files,
            &mut objstore,
            path,
        );

        Ok(())
    }

    pub fn commit(&mut self) -> Result<ObjectIndex> {
        let mut mw = meta::Writer::new(
            self.master_key.root_object_id()?,
            self.backend.clone(),
            self.master_key.get_meta_crypto()?,
        )?;

        mw.write_field(&self.files);
        mw.write_field(&self.chunks);
        mw.seal_and_store();

        Ok(mw.objects().clone())
    }

    pub fn file_index(&self) -> &files::FileIndex {
        self.files.index()
    }

    pub fn chunk_index(&self) -> &chunks::ChunkIndex {
        self.chunks.index()
    }
}
