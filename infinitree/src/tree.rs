use crate::{
    index::{
        self,
        fields::{self, QueryIteratorOwned},
        Access, Collection, Index, IndexExt, Load, ObjectIndex, QueryAction, Select, Serialized,
        Store,
    },
    object::{AEADReader, AEADWriter},
    Backend, Key, ObjectId,
};
use anyhow::{Context, Result};
use std::sync::Arc;

#[derive(Default, crate::Index)]
struct RootIndex {
    objects: ObjectIndex,
    last_written: Serialized<Option<ObjectId>>,
}

pub struct Infinitree<I> {
    root: RootIndex,
    index: I,

    backend: Arc<dyn Backend>,
    master_key: Key,
}

impl<I: Index + Default> Infinitree<I> {
    pub fn empty(backend: Arc<dyn Backend>, master_key: Key) -> Self {
        Self::with_key(backend, I::default(), master_key)
    }

    pub fn open(backend: Arc<dyn Backend>, master_key: Key) -> Result<Self> {
        let root_object = master_key.root_object_id()?;
        let mut root = RootIndex::default();

        open_root(&mut root, backend.clone(), &master_key, root_object)?;

        Ok(Self {
            root,
            index: I::default(),
            backend,
            master_key,
        })
    }
}

fn open_root<I: Index>(
    root: &mut I,
    backend: Arc<dyn Backend>,
    master_key: &Key,
    root_object: ObjectId,
) -> Result<()> {
    let mut reader = index::Reader::new(backend.clone(), master_key.get_meta_key()?);

    root.load_all_from(
        root_object,
        &mut reader,
        &mut AEADReader::new(backend.clone(), master_key.get_object_key()?),
    )?;

    Ok(())
}

fn merge_object_index(base: ObjectIndex, changeset: ObjectIndex) {
    // Note: This must be safe to unwrap, as we expect changeset to be
    // a unique object.
    changeset.for_each(|key, value| {
        base.upsert(
            key.to_owned(),
            || value.clone(),
            |_, v| {
                let v_mut = Arc::make_mut(v);
                v_mut.extend(value.iter());
                v_mut.dedup();
            },
        )
    });
}

impl<I: Index> Infinitree<I> {
    pub fn with_key(backend: Arc<dyn Backend>, index: I, master_key: Key) -> Self {
        Self {
            root: RootIndex::default(),
            backend,
            index,
            master_key,
        }
    }

    pub fn load_all(&mut self) -> Result<()> {
        let mut index = self.meta_reader()?; // TODO WTF
        let mut object = self.object_reader()?; // TODO WTF

        for mut action in self.index.load_all()?.drain(..) {
            for oid in Arc::make_mut(
                &mut self
                    .root
                    .objects
                    .read(&action.name, |_, v| v.clone())
                    .unwrap_or_default(),
            )
            .drain(..)
            {
                self.index.load(oid, &mut index, &mut object, &mut action);
            }
        }

        Ok(())
    }

    pub fn commit(&mut self) -> Result<()> {
        let key = self.master_key.get_meta_key()?;
        let start_meta = self
            .root
            .last_written
            .unwrap_or_else(|| ObjectId::new(&key));

        let mut index = index::Writer::new(start_meta, self.backend.clone(), key.clone())?;
        let mut object = self.object_writer()?;

        let changeset = self.index.commit(&mut index, &mut object)?;

        merge_object_index(self.root.objects.clone(), changeset);

        let mut index =
            index::Writer::new(self.master_key.root_object_id()?, self.backend.clone(), key)?;

        // ok to discard this as we're flushing the whole root object anyway
        let _ = self.root.commit(&mut index, &mut object)?;
        Ok(())
    }

    fn store_start_object(&self, _name: &str) -> ObjectId {
        ObjectId::new(&self.master_key.get_meta_key().unwrap())
    }

    fn query_start_object(&self, name: &str) -> Option<ObjectId> {
        self.root
            .objects
            .read(name, |_, v| v.first().cloned())
            .flatten()
    }

    pub fn store(&self, field: impl Into<Access<Box<dyn Store>>>) -> Result<()> {
        let mut field = field.into();
        let start_object = self.store_start_object(&field.name);

        self.index.store(
            &mut self.meta_writer(start_object)?,
            &mut self.object_writer()?,
            &mut field,
        );
        Ok(())
    }

    pub fn load(&self, field: impl Into<Access<Box<dyn Load>>>) -> Result<()> {
        let mut field = field.into();

        self.index.load(
            self.query_start_object(&field.name)
                .context("Empty index")?,
            &mut self.meta_reader()?,
            &mut self.object_reader()?,
            &mut field,
        );
        Ok(())
    }

    pub fn select<K>(
        &self,
        field: Access<Box<impl Select<Key = K>>>,
        pred: impl Fn(&K) -> QueryAction,
    ) -> Result<()> {
        self.index.query(
            self.query_start_object(&field.name)
                .context("Empty index")?,
            &mut self.meta_reader()?,
            &mut self.object_reader()?,
            field,
            pred,
        );
        Ok(())
    }

    pub fn query<K, O, Q>(
        &self,
        mut field: Access<Box<Q>>,
        pred: impl Fn(&K) -> QueryAction,
    ) -> Result<impl Iterator<Item = O>>
    where
        for<'de> <Q as fields::Collection>::Serialized: serde::Deserialize<'de>,
        Q: Collection<Key = K, Item = O>,
    {
        let index = self.meta_reader()?;
        let object = self.object_reader()?;
        let iter = QueryIteratorOwned::new(
            index
                .transaction(
                    &field.name,
                    &self
                        .query_start_object(&field.name)
                        .context("Empty index")?,
                )
                .unwrap(),
            object,
            pred,
            field.strategy.as_mut(),
        );

        Ok(iter)
    }

    fn meta_writer(&self, start_object: ObjectId) -> Result<index::Writer> {
        Ok(index::Writer::new(
            start_object,
            self.backend.clone(),
            self.master_key.get_meta_key()?,
        )?)
    }

    fn meta_reader(&self) -> Result<index::Reader> {
        Ok(index::Reader::new(
            self.backend.clone(),
            self.master_key.get_meta_key()?,
        ))
    }

    pub fn object_writer(&self) -> Result<AEADWriter> {
        Ok(AEADWriter::new(
            self.backend.clone(),
            self.master_key.get_object_key()?,
        ))
    }

    pub fn object_reader(&self) -> Result<AEADReader> {
        Ok(AEADReader::new(
            self.backend.clone(),
            self.master_key.get_object_key()?,
        ))
    }

    pub fn index(&self) -> &I {
        &self.index
    }
}
