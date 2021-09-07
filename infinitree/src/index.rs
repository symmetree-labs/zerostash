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

/// A collection to store object lists for fields
pub(crate) type ObjectIndex = Map<Field, Vec<ObjectId>>;

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

pub(crate) trait IndexExt: Index {
    fn load_all_from(
        &mut self,
        oid: ObjectId,
        index: &mut Reader,
        object: &mut dyn crate::object::Reader,
    ) -> anyhow::Result<()> {
        for mut action in self.load_all()?.drain(..) {
            self.load(oid, index, object, &mut action);
        }
        Ok(())
    }

    fn commit(
        &mut self,
        index: &mut Writer,
        object: &mut dyn crate::object::Writer,
    ) -> anyhow::Result<ObjectIndex> {
        let oi_update =
            self.store_all()?
                .drain(..)
                .fold(ObjectIndex::default(), |oi, mut action| {
                    oi.insert(
                        action.name.clone(),
                        self.store(index, object, &mut action).to_vec(),
                    );
                    oi
                });

        index.seal_and_store();
        Ok(oi_update)
    }

    fn store<'indexwriter>(
        &self,
        index: &'indexwriter mut Writer,
        object: &mut dyn crate::object::Writer,
        field: &mut Access<Box<dyn Store>>,
    ) -> &'indexwriter [ObjectId] {
        field
            .strategy
            .execute(index.transaction(&field.name), object);

        index.transaction_objects()
    }

    fn load(
        &self,
        oid: ObjectId,
        index: &mut Reader,
        object: &mut dyn crate::object::Reader,
        field: &mut Access<Box<dyn Load>>,
    ) {
        field
            .strategy
            .execute(index.transaction(&field.name, &oid).unwrap(), object);
    }

    fn query<K>(
        &self,
        oid: ObjectId,
        index: &mut Reader,
        object: &mut dyn crate::object::Reader,
        mut field: Access<Box<impl Query<Key = K>>>,
        pred: impl Fn(&K) -> QueryAction,
    ) {
        field
            .strategy
            .execute(index.transaction(&field.name, &oid).unwrap(), object, pred);
    }
}

impl<T> IndexExt for T where T: Index {}

/// Allows serializing an individual records of an infinite collection.
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
        let mut mw = super::Writer::new(oid, storage.clone(), crypto.clone()).unwrap();

        let chunks = ChunkIndex::default();
        chunks
            .entry(Digest::default())
            .or_insert_with(|| ChunkPointer::default());

        Store::execute(
            &mut LocalField::for_field(&chunks),
            mw.transaction("chunks"),
            &mut crate::object::AEADWriter::new(storage.clone(), crypto.clone()),
        );

        mw.seal_and_store();
        let objects = mw.transaction_objects();
        assert_eq!(objects.len(), 1);

        let chunks_restore = ChunkIndex::default();
        let mut reader = crate::object::AEADReader::new(storage.clone(), crypto.clone());

        // this runs once according to the assert above
        for id in objects.iter() {
            Load::execute(
                &mut LocalField::for_field(&chunks_restore),
                super::Reader::new(storage.clone(), crypto.clone())
                    .transaction("chunks", id)
                    .unwrap(),
                &mut reader,
            );
        }

        assert_eq!(chunks_restore.len(), 1);
    }
}
