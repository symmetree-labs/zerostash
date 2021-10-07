//! Working with the index of an Infinitree.
//!
//! # Index objects and regular objects
//!
//! # Efficient use of indexes
//!
//! # Customizing storage strategies

use crate::{
    compress,
    crypto::Digest,
    object::{ObjectId, WriteObject},
    ChunkPointer,
};
use serde::{de::DeserializeOwned, Serialize};
use std::{error::Error, io::Cursor};

pub mod fields;
mod header;
pub mod reader;
pub mod writer;

pub use fields::*;
pub use header::*;
pub use reader::ReadError;

pub(crate) use reader::Reader;
pub(crate) use writer::Writer;

/// A representation of a generation within the tree
pub(crate) type Generation = Digest;

/// A list of transactions, represented in order, for versions and fields
pub(crate) type TransactionList = Vec<(Generation, Field, ObjectId)>;

/// A collection to find a hash using a chunk location pointer
pub type ChunkIndex = Map<Digest, ChunkPointer>;

type Encoder = compress::Encoder<WriteObject>;
type Decoder =
    crate::Deserializer<rmp_serde::decode::ReadReader<compress::Decoder<Cursor<Vec<u8>>>>>;

/// Any structure that is usable as an Index
///
/// The two mandatory functions, [`store_all`](Index::store_all) and
/// [`load_all`][Index::load_all] are automatically generated if the
/// [`derive@crate::Index`] macro is used to derive this trait.
///
/// Generally an index will allow you to work with its fields
/// independently and in-memory, and the functions of this trait will
/// only help accessing backing storage. The [`Access`] instances wrap
/// each field in a way that an [`Infinitree`](crate::Infinitree) can work with.
pub trait Index: Send + Sync {
    /// Generate an [`Access`] wrapper for each field in the `Index`.
    ///
    /// You should normally use the [`Index`](derive@crate::Index) derive macro to generate this.
    fn store_all(&mut self) -> anyhow::Result<Vec<Access<Box<dyn Store>>>>;

    /// Generate an [`Access`] wrapper for each field in the `Index`.
    ///
    /// You should normally use the [`Index`](derive@crate::Index) derive macro to generate this.
    fn load_all(&mut self) -> anyhow::Result<Vec<Access<Box<dyn Load>>>>;
}

/// Allows serializing individual records of an infinite collection.
///
/// Implemented by a [`writer::Transaction`]. There's no need to implement this yourself.
pub trait FieldWriter: Send {
    /// Write the next `obj` into the index
    fn write_next(&mut self, obj: impl Serialize + Send);
}

/// Allows deserializing an infinite collection by reading records one by one.
///
/// Implemented by a [`reader::Transaction`]. There's no need to implement this yourself.
pub trait FieldReader: Send {
    /// Read the next available record from storage.
    fn read_next<T: DeserializeOwned>(&mut self) -> Result<T, Box<dyn Error>>;
}

impl FieldReader for Decoder {
    fn read_next<T: DeserializeOwned>(&mut self) -> Result<T, Box<dyn Error>> {
        Ok(T::deserialize(self)?)
    }
}

impl<T> IndexExt for T where T: Index {}

/// This is just a convenience layer to handle direct operations on an index
///
/// All of these functions are mirrored in [`Infinitree`] in a way
/// that's automatically handling reader/writer management & versions
///
/// In the future it may be worth exposing this more low-level interface
pub(crate) trait IndexExt: Index {
    fn load_all_from(
        &mut self,
        full_transaction_list: &TransactionList,
        index: &Reader,
        object: &mut dyn crate::object::Reader,
    ) -> anyhow::Result<()> {
        // #accidentallyquadratic

        for action in self.load_all()?.iter_mut() {
            let commits_for_field = full_transaction_list
                .iter()
                .filter(|(_, name, _)| name == &action.name)
                .cloned()
                .collect::<Vec<_>>();

            self.load(commits_for_field, index, object, action);
        }
        Ok(())
    }

    fn commit(
        &mut self,
        index: &mut Writer,
        object: &mut dyn crate::object::Writer,
    ) -> anyhow::Result<(Generation, Vec<(Field, ObjectId)>)> {
        let log = self
            .store_all()?
            .drain(..)
            .map(|mut action| (action.name.clone(), self.store(index, object, &mut action)))
            .collect();

        let version = crate::crypto::secure_hash(&crate::serialize_to_vec(&log)?);

        index.seal_and_store();
        Ok((version, log))
    }

    fn store<'indexwriter>(
        &self,
        index: &'indexwriter mut Writer,
        object: &mut dyn crate::object::Writer,
        field: &mut Access<Box<dyn Store>>,
    ) -> ObjectId {
        let mut tr = index.transaction(&field.name);

        field.strategy.execute(&mut tr, object);

        tr.finish()
    }

    fn load(
        &self,
        commits_for_field: TransactionList,
        index: &Reader,
        object: &mut dyn crate::object::Reader,
        field: &mut Access<Box<dyn Load>>,
    ) {
        field.strategy.load(index, object, commits_for_field);
    }

    fn select<K>(
        &self,
        commits_for_field: TransactionList,
        index: &Reader,
        object: &mut dyn crate::object::Reader,
        mut field: Access<Box<impl Select<Key = K>>>,
        pred: impl Fn(&K) -> QueryAction,
    ) {
        field
            .strategy
            .select(index, object, commits_for_field, pred);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn can_deserialize_fields() {
        use crate::backends;
        use crate::crypto::{self, Digest};
        use crate::index::*;
        use crate::object::ObjectId;
        use crate::ChunkPointer;

        use secrecy::Secret;
        use std::sync::Arc;

        let key = Secret::new(*b"abcdef1234567890abcdef1234567890");

        let crypto = crypto::ObjectOperations::new(key);
        let storage = Arc::new(backends::test::InMemoryBackend::default());
        let oid = ObjectId::new(&crypto);

        let chunks = ChunkIndex::default();
        chunks.insert(Digest::default(), ChunkPointer::default());

        let object = {
            let mut mw = super::Writer::new(oid, storage.clone(), crypto.clone()).unwrap();
            let mut transaction = mw.transaction("chunks");
            Store::execute(
                &mut LocalField::for_field(&chunks),
                &mut transaction,
                &mut crate::object::AEADWriter::new(storage.clone(), crypto.clone()),
            );

            let obj = transaction.finish();
            mw.seal_and_store();

            obj
        };

        let chunks_restore = ChunkIndex::default();
        let mut reader = crate::object::AEADReader::new(storage.clone(), crypto.clone());

        Load::load(
            &mut LocalField::for_field(&chunks_restore),
            &mut super::Reader::new(storage.clone(), crypto.clone()),
            &mut reader,
            vec![(Digest::default(), "chunks".into(), object)],
        );

        assert_eq!(chunks_restore.len(), 1);
    }
}
