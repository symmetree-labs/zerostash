use super::{writer, Collection, Key, LocalField, SparseField, Store, Value};
use crate::{
    index::FieldWriter,
    object::{self, serializer::SizedPointer, ObjectError},
};
use scc::HashMap;
use std::{borrow::Borrow, hash::Hash, sync::Arc};

/// A multithreaded map implementation that can be freely copied and
/// used with internal mutability across all operations.
#[derive(Clone, Default)]
pub struct Map<K: 'static + Key, V: 'static + Value>(Arc<HashMap<K, Arc<V>>>);

impl<K, V> Map<K, V>
where
    K: Key,
    V: Value,
{
    /// Insert a new value for the given key in the map.
    #[inline(always)]
    pub fn insert(&self, key: K, value: impl Into<Arc<V>>) -> Arc<V> {
        match self.get(&key) {
            Some(existing) => existing,
            None => {
                let new: Arc<_> = value.into();
                let _ = self.0.insert(key, new.clone());
                new
            }
        }
    }

    /// Update a value with the given key, and return the new value if
    /// the key exists.
    #[inline(always)]
    pub fn update_with<Q, R>(&self, key: &Q, fun: impl FnOnce(&K, &mut Arc<V>) -> R) -> Option<R>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.0.update(key.borrow(), fun)
    }

    /// Call the given function to insert a value if it doesn't exist.
    /// Return with the current value to the key.
    #[inline(always)]
    pub fn insert_with(&self, key: K, mut fun: impl FnMut() -> V) -> Arc<V> {
        match self.get(&key) {
            Some(existing) => existing,
            None => {
                let new: Arc<_> = (fun)().into();
                self.insert(key, new.clone());
                new
            }
        }
    }

    /// Returns the stored value for a key, or `None`.
    #[inline(always)]
    pub fn get<Q>(&self, key: &Q) -> Option<Arc<V>>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.0.read(key, |_, v| v.clone())
    }

    /// Sets the key as removed in the map.
    #[inline(always)]
    pub fn remove<Q>(&self, key: &Q)
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.0.remove(key);
    }

    /// Returns if there's an addition for the specified key.
    #[inline(always)]
    pub fn contains<Q>(&self, key: &Q) -> bool
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.0.contains(key)
    }

    /// Call the function for all additive keys.
    #[inline(always)]
    pub fn for_each(&self, mut callback: impl FnMut(&K, &mut Arc<V>)) {
        self.0.for_each(|k, v| {
            (callback)(k, v);
        });
    }

    /// Returns the number of keys.
    #[inline(always)]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Return the size of all allocated items.
    #[inline(always)]
    pub fn capacity(&self) -> usize {
        self.0.capacity()
    }

    /// True if the map doesn't contain any items.
    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.0.len() == 0
    }
}

impl<'index, K, V> Store for LocalField<Map<K, V>>
where
    K: Key,
    V: Value,
{
    fn execute(&mut self, transaction: &mut writer::Transaction, _object: &mut dyn object::Writer) {
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
    type TransactionResolver = super::FirstOnly;
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
    fn execute(&mut self, transaction: &mut writer::Transaction, writer: &mut dyn object::Writer) {
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
    type TransactionResolver = super::FirstOnly;
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
