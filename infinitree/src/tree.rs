use crate::{
    index::{
        self,
        fields::{self, QueryIteratorOwned, Serialized},
        Access, Collection, Generation, Index, IndexExt, Load, QueryAction, Select, Store,
        TransactionList, TransactionResolver,
    },
    object::{AEADReader, AEADWriter},
    Backend, Key, ObjectId,
};
use anyhow::Result;
use chrono::{DateTime, Utc};
use std::sync::Arc;

#[derive(Serialize, Deserialize, Debug, Clone)]
struct CommitMetadata {
    generation: Generation,
    message: Option<String>,
    time: DateTime<Utc>,
}

#[derive(Default, crate::Index)]
struct RootIndex {
    /// Transaction log of individual fields included in each
    /// generation.
    ///
    /// The last generation's transactions are at _the front_, so
    /// looping through this naively will yield the last commit
    /// _first_.
    transaction_log: Serialized<TransactionList>,

    /// Chronologically ordered list of commits
    commit_metadata: Serialized<Vec<CommitMetadata>>,
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
            backend,
            master_key,
            index: I::default(),
        })
    }
}

fn open_root(
    root: &mut RootIndex,
    backend: Arc<dyn Backend>,
    master_key: &Key,
    root_object: ObjectId,
) -> Result<()> {
    let reader = index::Reader::new(backend.clone(), master_key.get_meta_key()?);

    root.load_all_from(
        &root
            .fields()
            .iter()
            .cloned()
            .map(|fname| (crate::Digest::default(), fname, root_object))
            .collect::<TransactionList>(),
        &reader,
        &mut AEADReader::new(backend.clone(), master_key.get_object_key()?),
    )?;

    println!("restored: {:?}", root.transaction_log.read());

    Ok(())
}

impl<I: Index> Infinitree<I> {
    pub fn with_key(backend: Arc<dyn Backend>, index: I, master_key: Key) -> Self {
        Self {
            backend,
            index,
            master_key,
            root: RootIndex::default(),
        }
    }

    pub fn load_all(&mut self) -> Result<()> {
        self.index.load_all_from(
            &self.root.transaction_log.read(),
            &self.meta_reader()?,
            &mut self.object_reader()?,
        )
    }

    pub fn commit(&mut self, message: Option<String>) -> Result<()> {
        let key = self.master_key.get_meta_key()?;
        let start_meta = ObjectId::new(&key);

        let mut index = index::Writer::new(start_meta, self.backend.clone(), key.clone())?;
        let mut object = self.object_writer()?;

        let (generation, changeset) = self.index.commit(&mut index, &mut object)?;

        // scope for rewriting history. this is critical, the log is locked.
        {
            let mut tr_log = self.root.transaction_log.write();
            let size = tr_log.len() + changeset.len();
            let history = std::mem::replace(&mut *tr_log, Vec::with_capacity(size));

            tr_log.extend(
                changeset
                    .into_iter()
                    .map(|(field, oid)| (generation, field, oid)),
            );
            tr_log.extend(history);
        }

        self.root.commit_metadata.write().push(CommitMetadata {
            generation,
            message,
            time: Utc::now(),
        });

        let mut index =
            index::Writer::new(self.master_key.root_object_id()?, self.backend.clone(), key)?;

        // ok to discard this as we're flushing the whole root object anyway
        let _ = self.root.commit(&mut index, &mut object)?;
        Ok(())
    }

    pub fn store(&self, field: impl Into<Access<Box<dyn Store>>>) -> Result<ObjectId> {
        let mut field = field.into();
        let start_object = self.store_start_object(&field.name);

        Ok(self.index.store(
            &mut self.meta_writer(start_object)?,
            &mut self.object_writer()?,
            &mut field,
        ))
    }

    pub fn load(&self, field: impl Into<Access<Box<dyn Load>>>) -> Result<()> {
        let mut field = field.into();
        let commits_for_field = self.field_for_version(&field.name);

        field.strategy.load(
            &self.meta_reader()?,
            &mut self.object_reader()?,
            commits_for_field,
        );

        Ok(())
    }

    pub fn select<K>(
        &self,
        mut field: Access<Box<impl Select<Key = K>>>,
        pred: impl Fn(&K) -> QueryAction,
    ) -> Result<()> {
        let commits_for_field = self.field_for_version(&field.name);

        field.strategy.select(
            &self.meta_reader()?,
            &mut self.object_reader()?,
            commits_for_field,
            pred,
        );

        Ok(())
    }

    pub fn query<K, O, Q>(
        &self,
        mut field: Access<Box<Q>>,
        pred: impl Fn(&K) -> QueryAction + 'static,
    ) -> Result<impl Iterator<Item = O> + '_>
    where
        for<'de> <Q as fields::Collection>::Serialized: serde::Deserialize<'de>,
        Q: Collection<Key = K, Item = O> + 'static,
    {
        let pred = Arc::new(pred);
        let index = self.meta_reader()?;
        let object = self.object_reader()?;
        let commits_for_field = self.field_for_version(&field.name);

        Ok(
            <Q as Collection>::TransactionResolver::resolve(index, commits_for_field)
                .map(move |transaction| {
                    QueryIteratorOwned::new(
                        transaction,
                        object.clone(),
                        pred.clone(),
                        field.strategy.as_mut(),
                    )
                })
                .flatten(),
        )
    }

    fn store_start_object(&self, _name: &str) -> ObjectId {
        ObjectId::new(&self.master_key.get_meta_key().unwrap())
    }

    fn field_for_version(&self, field: &index::Field) -> TransactionList {
        self.root
            .transaction_log
            .read()
            .iter()
            .filter(|(_, name, _)| name == field)
            .cloned()
            .collect::<Vec<_>>()
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
