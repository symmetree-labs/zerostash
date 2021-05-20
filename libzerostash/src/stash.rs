use crate::{
    backends::Backend,
    crypto::StashKey,
    index::{self, Index, ObjectIndex},
    object::{AEADReader, AEADWriter},
};
use std::sync::Arc;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub struct Stash<I> {
    backend: Arc<dyn Backend>,
    index: I,
    master_key: StashKey,
}

impl<I: Index + Default> Stash<I> {
    pub fn with_default_index(backend: Arc<dyn Backend>, master_key: StashKey) -> Self {
        Self::new(backend, master_key, I::default())
    }
}

impl<I: Index> Stash<I> {
    pub fn new(backend: Arc<dyn Backend>, master_key: StashKey, index: I) -> Self {
        Stash {
            backend,
            index,
            master_key,
        }
    }

    pub async fn read(&mut self) -> Result<&Self> {
        let metareader =
            index::Reader::new(self.backend.clone(), self.master_key.get_meta_crypto()?);
        let start_object = self.master_key.root_object_id()?;

        self.index.read_fields(metareader, start_object).await?;
        Ok(self)
    }

    pub async fn commit(&mut self) -> Result<ObjectIndex> {
        let mut mw = index::Writer::new(
            self.master_key.root_object_id()?,
            self.backend.clone(),
            self.master_key.get_meta_crypto()?,
        )?;

        self.index.write_fields(&mut mw).await?;

        Ok(mw.objects().clone())
    }

    pub fn object_writer(&self) -> Result<AEADWriter> {
        Ok(AEADWriter::new(
            self.backend.clone(),
            self.master_key.get_object_crypto()?,
        ))
    }

    pub fn object_reader(&self) -> Result<AEADReader> {
        Ok(AEADReader::new(
            self.backend.clone(),
            self.master_key.get_object_crypto()?,
        ))
    }

    pub fn index(&self) -> &I {
        &self.index
    }
}
