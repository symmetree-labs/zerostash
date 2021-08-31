use crate::{
    backends::Backend,
    crypto::StashKey,
    index::{self, Access, Index, Load, ObjectIndex, Query, QueryAction, Store},
    object::{AEADReader, AEADWriter},
};
use std::sync::Arc;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(crate::Index)]
struct RootIndex {
    // record object utilization
// record ObjectIndex for index
}

pub struct Stash<I> {
    root: RootIndex,
    pub(crate) backend: Arc<dyn Backend>,
    pub(crate) index: I,
    pub(crate) master_key: StashKey,
}

impl<I: Index + Default> Stash<I> {
    pub fn with_default_index(backend: Arc<dyn Backend>, master_key: StashKey) -> Self {
        Self::new(backend, master_key, I::default())
    }
}

impl<I: Index> Stash<I> {
    pub fn new(backend: Arc<dyn Backend>, master_key: StashKey, index: I) -> Self {
        Stash {
            root: RootIndex {},
            backend,
            index,
            master_key,
        }
    }

    pub async fn load_all(&mut self) -> Result<()> {
        for action in self.index.load_all()?.drain(..) {
            self.load(action);
        }
        Ok(())
    }

    pub async fn commit(&mut self) -> Result<()> {
        for action in self.index.store_all()?.drain(..) {
            self.store(action);
        }
        Ok(())
    }

    fn meta_writer(&self) -> Result<index::Writer> {
        Ok(index::Writer::new(
            self.master_key.root_object_id()?,
            self.backend.clone(),
            self.master_key.get_meta_crypto()?,
        )?)
    }

    fn meta_reader(&self) -> Result<index::Reader> {
        Ok(index::Reader::new(
            self.backend.clone(),
            self.master_key.get_meta_crypto()?,
        ))
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

    pub fn store(&self, mut field: impl Into<Access<Box<dyn Store>>>) {
        field.into().strategy.execute(
            Arc::new(self.meta_writer().unwrap()),
            &self.object_writer().unwrap(),
        )
    }

    pub fn load(&self, mut field: impl Into<Access<Box<dyn Load>>>) {
        field.into().strategy.execute()
    }

    pub fn query<K>(&self, mut field: Access<Box<impl Query<K>>>, pred: impl Fn(K) -> QueryAction) {
        field.strategy.execute(pred)
    }

    pub fn index(&self) -> &I {
        &self.index
    }
}
