use super::{Collection, Key, LocalField, Map, SparseField, Store, Strategy, Value};
use crate::{
    index::{writer, FieldWriter},
    object::{self, serializer::SizedPointer, ObjectError},
};
use std::sync::Arc;

type RawAction<V> = Option<V>;
type Action<V> = Option<Arc<V>>;

fn store<V>(value: V) -> Action<V> {
    Some(Arc::new(value))
}

#[derive(Clone)]
pub struct VersionedMap<K, V>
where
    K: Key + 'static,
    V: Value + 'static,
{
    current: Map<K, Action<V>>,
    base: Map<K, Action<V>>,
}

impl<K, V> Default for VersionedMap<K, V>
where
    K: Key,
    V: Value,
{
    fn default() -> Self {
        Self {
            current: Map::default(),
            base: Map::default(),
        }
    }
}

impl<K, V> VersionedMap<K, V>
where
    K: Key,
    V: Value,
{
    /// Set or overwrite a value for the given key in the map
    #[inline(always)]
    pub fn insert(&self, key: K, value: V) {
        let new_value = store(value);
        let new_ref = new_value.clone();

        self.current.upsert(key, || new_value, |_, v| *v = new_ref);
    }

    /// Call a function to set or overwrite the value at the given
    /// `key`
    #[inline(always)]
    pub fn insert_with(&self, key: K, mut fun: impl FnMut() -> V) {
        self.current.upsert(key, || store((fun)()), |_, _| {});
    }

    /// Returns the stored value for a key, or `None`
    #[inline(always)]
    pub fn get(&self, key: &K) -> Option<Arc<V>> {
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
                (callback)(k, Arc::as_ref(&value));
            }
        });
        self.current.for_each(|k, v: &mut Action<V>| {
            if let Some(value) = v {
                (callback)(k, Arc::as_ref(&value));
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
}

impl<K, V> Collection for LocalField<VersionedMap<K, V>>
where
    K: Key,
    V: Value,
{
    type Key = <LocalField<Map<K, Action<V>>> as Collection>::Key;
    type Serialized = <LocalField<Map<K, Action<V>>> as Collection>::Serialized;
    type Item = <LocalField<Map<K, Action<V>>> as Collection>::Item;

    #[inline(always)]
    fn key(from: &Self::Serialized) -> &Self::Key {
        <LocalField<Map<K, Action<V>>> as Collection>::key(from)
    }

    #[inline(always)]
    fn load(from: Self::Serialized, object: &mut dyn crate::object::Reader) -> Self::Item {
        <LocalField<Map<K, Action<V>>> as Collection>::load(from, object)
    }

    #[inline(always)]
    fn insert(&mut self, record: Self::Item) {
        <LocalField<Map<K, Action<V>>> as Collection>::insert(
            &mut LocalField::for_field(&self.field.base),
            record,
        )
    }
}

impl<K, V> Store for LocalField<VersionedMap<K, V>>
where
    K: Key,
    V: Value,
{
    #[inline(always)]
    fn execute(&mut self, mut transaction: writer::Transaction, _object: &mut dyn object::Writer) {
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
            Some(value) => {
                let value = object::serializer::read(
                    object,
                    |x| {
                        crate::deserialize_from_slice(x).map_err(|e| ObjectError::Deserialize {
                            source: Box::new(e),
                        })
                    },
                    value,
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
        match record.1 {
            value @ Some(..) => {
                let _ = self.field.base.insert(record.0, value.clone());
            }
            None => (),
        }
    }
}

impl<K, V> Store for SparseField<VersionedMap<K, V>>
where
    K: Key,
    V: Value,
{
    #[inline(always)]
    fn execute(&mut self, mut transaction: writer::Transaction, writer: &mut dyn object::Writer) {
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
