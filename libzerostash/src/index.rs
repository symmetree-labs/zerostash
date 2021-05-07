use crate::{
    meta::{self, FieldReader, FieldWriter},
    object::ObjectId,
};

use async_trait::async_trait;
use dashmap::{DashMap, DashSet};
use serde::{de::DeserializeOwned, Serialize};

use std::{cmp::Eq, hash::Hash, sync::Arc};

#[async_trait]
pub trait IndexField {
    type Item: DeserializeOwned;

    async fn serialize(&self, mw: &mut impl FieldWriter);
    async fn deserialize(&self, mw: &mut impl FieldReader<Self::Item>);
}

#[async_trait]
pub trait Index {
    async fn read_fields(
        &mut self,
        metareader: meta::Reader,
        start_object: ObjectId,
    ) -> Result<(), Box<dyn std::error::Error>>;

    async fn write_fields(
        &mut self,
        metareader: &mut meta::Writer,
    ) -> Result<(), Box<dyn std::error::Error>>;
}

pub trait Value: Serialize + DeserializeOwned + Send + Sync {}
pub trait Key: Serialize + DeserializeOwned + Eq + Hash + Send + Sync {}

impl<T> Value for T where T: Serialize + DeserializeOwned + Send + Sync {}
impl<T> Key for T where T: Serialize + DeserializeOwned + Eq + Hash + Send + Sync {}

pub type Set<V: Key> = Arc<DashSet<V>>;

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

pub type Map<K: Key, V: Value> = Arc<DashMap<K, V>>;

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
