use super::{Collection, Key, LocalField, SparseField, Store, Value};
use crate::{
    index::{writer, FieldWriter},
    object::{self, serializer::SizedPointer, ObjectError},
};
use scc::HashMap;
use std::{borrow::Borrow, cell::Cell, hash::Hash, sync::Arc};

type RawAction<V> = Option<V>;
type Action<V> = Option<Arc<V>>;

fn store<V>(value: impl Into<Arc<V>>) -> Action<V> {
    Some(value.into())
}

fn store_if_none<V>(current: &mut Option<Arc<V>>, value: impl Into<Arc<V>>) {
    current.get_or_insert(value.into());
}

#[derive(Clone, Default)]
pub struct VersionedMap<K, V>
where
    K: Key + 'static,
    V: Value + 'static,
{
    current: Arc<HashMap<K, Action<V>>>,
    base: Arc<HashMap<K, Action<V>>>,
}

impl<K, V> VersionedMap<K, V>
where
    K: Key,
    V: Value,
{
    /// Set or overwrite a value for the given key in the map
    #[inline(always)]
    pub fn insert(&self, key: K, value: impl Into<Arc<V>>) -> Arc<V> {
        match self.get(&key) {
            Some(v) => v,
            None => {
                let new = value.into();

                self.current.upsert(
                    key,
                    || store(new.clone()),
                    |_, v| store_if_none(v, new.clone()),
                );
                new
            }
        }
    }

    /// Call a function to set or overwrite the value at the given
    /// `key`
    #[inline(always)]
    pub fn insert_with(&self, key: K, new: impl Fn() -> V) -> Arc<V> {
        match self.get(&key) {
            Some(v) => v,
            None => {
                let result = Cell::new(None);

                self.current.upsert(
                    key,
                    || {
                        let val = store(new());
                        result.set(val.clone());
                        val
                    },
                    |_, v| {
                        *v = store(new());
                        result.set(v.clone())
                    },
                );

                // this will never panic, because callbacks guarantee it ends up being Some()
                result.into_inner().unwrap()
            }
        }
    }

    #[inline(always)]
    pub fn update_with(&self, key: K, new: impl Fn(Arc<V>) -> V) -> Action<V> {
        match self.get(&key) {
            Some(existing) => {
                let result = Cell::new(None);

                self.current.upsert(
                    key,
                    || {
                        let val = store(new(existing.clone()));
                        result.set(val.clone());
                        val
                    },
                    |_, v| {
                        *v = store(new(v.as_ref().unwrap().clone()));
                        result.set(v.clone())
                    },
                );

                // this will never panic, because callbacks guarantee it ends up being Some()
                result.into_inner()
            }
            None => None,
        }
    }

    /// Returns the stored value for a key, or `None`
    #[inline(always)]
    pub fn get<Q>(&self, key: &Q) -> Option<Arc<V>>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.current
            .read(key, |_, v| v.clone())
            .or_else(|| self.base.read(key, |_, v| v.clone()))
            .flatten()
    }

    /// Sets the key as removed in the map
    #[inline(always)]
    pub fn remove(&self, key: K) {
        if self.contains(&key) {
            self.current
                .upsert(key, || Action::None, |_, v| *v = Action::None)
        }
    }

    /// Returns if there's an addition for the specified key
    #[inline(always)]
    pub fn contains(&self, key: &K) -> bool {
        let contained = self
            .current
            .read(key, |_, v| v.is_some())
            .or_else(|| self.base.read(key, |_, v| v.is_some()));

        contained.unwrap_or(false)
    }

    /// Call the function for all additive keys
    #[inline(always)]
    pub fn for_each(&self, mut callback: impl FnMut(&K, &V)) {
        // note: this is copy-pasta, because the closures have
        // different lifetimes.
        //
        // if you have a good idea how to avoid
        // using a macro and just do this, please send a PR

        self.base.for_each(|k, v: &mut Action<V>| {
            if let Some(value) = v {
                (callback)(k, Arc::as_ref(value));
            }
        });
        self.current.for_each(|k, v: &mut Action<V>| {
            if let Some(value) = v {
                (callback)(k, Arc::as_ref(value));
            }
        });
    }

    /// Returns the number of additive keys
    #[inline(always)]
    pub fn len(&self) -> usize {
        let mut stored = 0;
        self.current.for_each(|_, v| {
            if v.is_some() {
                stored += 1
            }
        });

        self.base.len() + stored
    }

    /// Returns the number of all keys, including deletions
    #[inline(always)]
    pub fn size(&self) -> usize {
        self.base.len() + self.current.len()
    }

    /// Return the size of all allocated items
    #[inline(always)]
    pub fn capacity(&self) -> usize {
        self.base.capacity() + self.current.capacity()
    }

    /// True if the number of additions to the map is zero
    ///
    /// Since `VersionedMap` is tracking _changes_, `is_empty()` may
    /// return `true` even if a non-zero amount of memory is being
    /// used.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl<K, V> Collection for LocalField<VersionedMap<K, V>>
where
    K: Key,
    V: Value,
{
    type TransactionResolver = super::FullHistory;
    type Key = K;
    type Serialized = (K, Action<V>);
    type Item = (K, Action<V>);

    #[inline(always)]
    fn key(from: &Self::Serialized) -> &Self::Key {
        &from.0
    }

    #[inline(always)]
    fn load(from: Self::Serialized, _object: &mut dyn crate::object::Reader) -> Self::Item {
        from
    }

    #[inline(always)]
    fn insert(&mut self, record: Self::Item) {
        debug_assert!(self.field.base.insert(record.0, record.1).is_ok());
    }
}

impl<K, V> Store for LocalField<VersionedMap<K, V>>
where
    K: Key,
    V: Value,
{
    #[inline(always)]
    fn execute(&mut self, transaction: &mut writer::Transaction, _object: &mut dyn object::Writer) {
        self.field.current.for_each(|k, v| {
            transaction.write_next((k, v));
        })
    }
}

impl<K, V> Collection for SparseField<VersionedMap<K, V>>
where
    K: Key,
    V: Value,
{
    type TransactionResolver = super::FullHistory;
    type Key = K;
    type Serialized = (K, RawAction<SizedPointer>);
    type Item = (K, Action<V>);

    #[inline(always)]
    fn key(from: &Self::Serialized) -> &Self::Key {
        &from.0
    }

    #[inline(always)]
    fn load(from: Self::Serialized, object: &mut dyn object::Reader) -> Self::Item {
        let value = match from.1 {
            Some(ptr) => {
                let value: V = object::serializer::read(
                    object,
                    |x| {
                        crate::deserialize_from_slice(x).map_err(|e| ObjectError::Deserialize {
                            source: Box::new(e),
                        })
                    },
                    ptr,
                )
                .unwrap();

                store(value)
            }
            None => None,
        };

        (from.0, value)
    }

    #[inline(always)]
    fn insert(&mut self, record: Self::Item) {
        // we're optimizing for the case where the versions are
        // restored from top to bottom, in reverse order.
        // therefore:
        // 1. do not insert a key if it already exists
        // 2. do not restore a removed key
        if let value @ Some(..) = record.1 {
            let _ = self.field.base.insert(record.0, value);
        }
    }
}

impl<K, V> Store for SparseField<VersionedMap<K, V>>
where
    K: Key,
    V: Value,
{
    #[inline(always)]
    fn execute(&mut self, transaction: &mut writer::Transaction, writer: &mut dyn object::Writer) {
        self.field.current.for_each(|key, value| {
            let ptr = value.as_ref().map(|stored| {
                object::serializer::write(
                    writer,
                    |x| {
                        crate::serialize_to_vec(&x).map_err(|e| ObjectError::Serialize {
                            source: Box::new(e),
                        })
                    },
                    stored,
                )
                .unwrap()
            });
            transaction.write_next((key, ptr));
        })
    }
}
