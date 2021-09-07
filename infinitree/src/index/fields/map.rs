use super::{reader, writer, Key, LocalField, Query, QueryAction, SparseField, Store, Value};
use crate::{
    index,
    index::{FieldReader, FieldWriter},
    object::{self, ObjectError},
};
use dashmap::DashMap;
use std::sync::Arc;

/// A multithreaded map implementation that can be freely copied and
/// used with internal mutability across all operations.
pub type Map<K, V> = Arc<DashMap<K, V>>;

impl<'index, K, V> Store for LocalField<Map<K, V>>
where
    K: Key,
    V: Value,
{
    fn execute(&mut self, mut transaction: writer::Transaction, _object: &mut dyn object::Writer) {
        for r in self.field.iter() {
            transaction.write_next((r.key(), r.value()));
        }
    }
}

impl<'index, K, V> Query for LocalField<Map<K, V>>
where
    K: Key,
    V: Value,
{
    type Key = K;

    fn execute(
        &mut self,
        mut transaction: index::reader::Transaction,
        _object: &mut dyn object::Reader,
        predicate: impl Fn(&K) -> QueryAction,
    ) {
        while let Ok((key, value)) = transaction.read_next() {
            use QueryAction::*;

            match (predicate)(&key) {
                Take => {
                    self.field.insert(key, value);
                }
                Skip => (),
                Abort => break,
            }
        }
    }
}

impl<K, V> Store for SparseField<Map<K, V>>
where
    K: Key,
    V: Value,
{
    fn execute(&mut self, mut transaction: writer::Transaction, writer: &mut dyn object::Writer) {
        for r in self.field.iter() {
            let ptr = object::serializer::write(
                writer,
                |x| {
                    crate::serialize_to_vec(x).map_err(|e| ObjectError::Serialize {
                        source: Box::new(e),
                    })
                },
                r.value(),
            )
            .unwrap();
            transaction.write_next((r.key(), ptr));
        }
    }
}

impl<K, V> Query for SparseField<Map<K, V>>
where
    K: Key,
    V: Value,
{
    type Key = K;

    fn execute(
        &mut self,
        mut transaction: reader::Transaction,
        object: &mut dyn object::Reader,
        predicate: impl Fn(&K) -> QueryAction,
    ) {
        while let Ok((key, ptr)) = transaction.read_next() {
            use QueryAction::*;

            match (predicate)(&key) {
                Take => {
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
                    self.field.insert(key, value);
                }
                Skip => (),
                Abort => break,
            }
        }
    }
}
