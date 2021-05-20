use super::{FieldReader, FieldWriter};
use std::{cmp::Eq, hash::Hash, sync::Arc};

use async_trait::async_trait;
use dashmap::{DashMap, DashSet};
use serde::{de::DeserializeOwned, Serialize};

pub trait Value: Serialize + DeserializeOwned + Send + Sync {}
pub trait Key: Serialize + DeserializeOwned + Eq + Hash + Send + Sync {}

#[async_trait]
pub trait IndexField {
    type Item: DeserializeOwned;

    async fn serialize(&self, mw: &mut impl FieldWriter);
    async fn deserialize(&self, mw: &mut impl FieldReader<Self::Item>);
}

impl<T> Value for T where T: Serialize + DeserializeOwned + Send + Sync {}
impl<T> Key for T where T: Serialize + DeserializeOwned + Eq + Hash + Send + Sync {}

pub type Set<V> = Arc<DashSet<V>>;

#[async_trait]
impl<V: Key> IndexField for Set<V> {
    type Item = V;

    async fn serialize(&self, mw: &mut impl FieldWriter) {
        for f in self.iter() {
            mw.write_next(f.key()).await;
        }
    }

    async fn deserialize(&self, mw: &mut impl FieldReader<Self::Item>) {
        while let Ok(item) = mw.read_next().await {
            self.insert(item);
        }
    }
}

pub type Map<K, V> = Arc<DashMap<K, V>>;

#[async_trait]
impl<K: Key, V: Value> IndexField for Map<K, V> {
    type Item = (K, V);

    async fn serialize(&self, mw: &mut impl FieldWriter) {
        for r in self.iter() {
            mw.write_next((r.key(), r.value())).await;
        }
    }

    async fn deserialize(&self, mw: &mut impl FieldReader<Self::Item>) {
        while let Ok((hash, pointer)) = mw.read_next().await {
            self.insert(hash, pointer);
        }
    }
}
