use super::{writer, Collection, Key, LocalField, SparseField, Store, Value};
use crate::{
    index::FieldWriter,
    object::{self, serializer::SizedPointer, ObjectError},
};
use scc::HashMap;
use std::sync::Arc;

/// A multithreaded map implementation that can be freely copied and
/// used with internal mutability across all operations.
pub type Map<K, V> = Arc<HashMap<K, V>>;

impl<'index, K, V> Store for LocalField<Map<K, V>>
where
    K: Key,
    V: Value,
{
    fn execute(&mut self, mut transaction: writer::Transaction, _object: &mut dyn object::Writer) {
        self.field.for_each(|k, v| {
            transaction.write_next((k, v));
        })
    }
}

impl<'index, K, V> Collection for LocalField<Map<K, V>>
where
    K: Key,
    V: Value,
{
    type Key = K;
    type Serialized = (K, V);
    type Item = (K, V);

    fn key(from: &Self::Serialized) -> &Self::Key {
        &from.0
    }

    fn load(from: Self::Serialized, _object: &mut dyn object::Reader) -> Self::Item {
        from
    }

    fn insert(&mut self, record: Self::Item) {
        self.field.insert(record.0, record.1);
    }
}

impl<K, V> Store for SparseField<Map<K, V>>
where
    K: Key,
    V: Value,
{
    fn execute(&mut self, mut transaction: writer::Transaction, writer: &mut dyn object::Writer) {
        self.field.for_each(|key, value| {
            let ptr = object::serializer::write(
                writer,
                |x| {
                    crate::serialize_to_vec(x).map_err(|e| ObjectError::Serialize {
                        source: Box::new(e),
                    })
                },
                value,
            )
            .unwrap();
            transaction.write_next((key, ptr));
        })
    }
}

impl<'index, K, V> Collection for SparseField<Map<K, V>>
where
    K: Key,
    V: Value,
{
    type Key = K;
    type Serialized = (K, SizedPointer);
    type Item = (K, V);

    fn key(from: &Self::Serialized) -> &Self::Key {
        &from.0
    }

    fn load(from: Self::Serialized, object: &mut dyn object::Reader) -> Self::Item {
        let (key, ptr) = from;

        let value = object::serializer::read(
            object,
            |x| {
                crate::deserialize_from_slice(x).map_err(|e| ObjectError::Deserialize {
                    source: Box::new(e),
                })
            },
            ptr,
        )
        .unwrap();

        (key, value)
    }

    fn insert(&mut self, record: Self::Item) {
        self.field.insert(record.0, record.1);
    }
}
