use crate::{backends::Backend, chunks, files, meta, objects};
pub use crate::{crypto::StashKey, meta::ObjectIndex};

use failure::Error;

use std::path::Path;

pub(crate) mod restore;
pub(crate) mod store;

pub struct Stash<B> {
    backend: B,
    chunks: chunks::ChunkStore,
    files: files::FileStore,
    master_key: StashKey,
}

impl<B> Stash<B>
where
    B: Backend,
{
    pub fn new(backend: B, master_key: StashKey) -> Stash<B> {
        let chunks = chunks::ChunkStore::default();
        let files = files::FileStore::default();

        Stash {
            backend,
            chunks,
            files,
            master_key,
        }
    }

    pub fn read(&mut self) -> Result<&Self, Error> {
        let mut metareader =
            meta::Reader::new(self.backend.clone(), self.master_key.get_meta_crypto()?);
        let mut next_object = Some(self.master_key.root_object_id()?);

        while let Some(header) = match next_object {
            Some(ref o) => Some(metareader.open(o)?),
            None => None,
        } {
            next_object = header.next_object();
            for field in header.fields().iter() {
                use self::meta::Field::*;
                match field {
                    Chunks => metareader.read_into(field, &mut self.chunks)?,
                    Files => metareader.read_into(field, &mut self.files)?,
                };
            }
        }

        Ok(self)
    }

    pub fn restore_by_glob(
        &mut self,
        threads: usize,
        pattern: impl AsRef<str>,
        target: impl AsRef<Path>,
    ) -> Result<(), Error> {
        restore::from_glob(
            pattern.as_ref(),
            threads,
            self.files.index(),
            &self.backend,
            self.master_key.get_object_crypto()?,
            target,
        )
    }

    pub fn add_recursive(&mut self, threads: usize, path: impl AsRef<Path>) -> Result<(), Error> {
        let mut objstore =
            objects::Storage::new(self.backend.clone(), self.master_key.get_object_crypto()?);

        store::recursive(
            threads,
            &mut self.chunks,
            &mut self.files,
            &mut objstore,
            path,
        )
    }

    pub fn commit(&mut self) -> Result<ObjectIndex, Error> {
        let mut mw = meta::Writer::new(
            self.master_key.root_object_id()?,
            self.backend.clone(),
            self.master_key.get_meta_crypto()?,
        )?;

        mw.write_field(meta::Field::Files, &self.files);
        mw.write_field(meta::Field::Chunks, &self.chunks);
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
