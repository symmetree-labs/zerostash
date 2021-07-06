use super::{Key, Load, LocalField, Query, QueryAction, SparseField, Store, Value};
use crate::{index, object};
use dashmap::DashMap;
use std::sync::Arc;

pub type Map<K, V> = Arc<DashMap<K, V>>;

impl<'index, K, V> Query<K> for LocalField<Map<K, V>>
where
    K: Key,
    V: Value,
{
    fn execute(&mut self, predicate: impl Fn(K) -> QueryAction) {
        todo!()
    }
}

impl<K, V> Store for SparseField<Map<K, V>>
where
    K: Key,
    V: Value,
{
    fn execute(&mut self, _meta: Arc<index::Writer>, _writer: &dyn object::Writer) {
        todo!()
    }
}

impl<K, V> Query<K> for SparseField<Map<K, V>>
where
    K: Key,
    V: Value,
{
    fn execute(&mut self, predicate: impl Fn(K) -> QueryAction) {
        todo!()
    }
}

impl<'index, K, V> Store for LocalField<Map<K, V>>
where
    K: Key,
    V: Value,
{
    fn execute(&mut self, _meta: Arc<index::Writer>, _writer: &dyn object::Writer) {
        todo!()
    }
}

impl<'index, K, V> Load for LocalField<Map<K, V>>
where
    K: Key,
    V: Value,
{
    fn execute(&mut self) {
        todo!()
    }
}

// impl<K: Key, V: Value> IndexField for Map<K, V> {
//     type Item = (K, V);

//     async fn serialize(&self, mw: &mut impl FieldWriter) {
//         for r in self.iter() {
//             mw.write_next((r.key(), r.value()));
//         }
//     }

//     async fn deserialize(&self, mw: &mut impl FieldReader<'_, Self::Item>) {
//         while let Ok((hash, pointer)) = mw.read_next() {
//             self.insert(hash, pointer);
//         }
//     }
// }
