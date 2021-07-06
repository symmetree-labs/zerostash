use super::{Key, Load, LocalField, Query, QueryAction, SparseField, Store, Value};
use crate::{index, object};
use dashmap::DashSet;
use std::sync::Arc;

pub type Set<V> = Arc<DashSet<V>>;

impl<'index, K> Query<K> for LocalField<Set<K>>
where
    K: Key,
{
    fn execute(&mut self, predicate: impl Fn(K) -> QueryAction) {
        todo!()
    }
}

impl<'index, K> Load for LocalField<Set<K>>
where
    K: Key,
{
    fn execute(&mut self) {
        todo!()
    }
}

impl<'index, K> Store for LocalField<Set<K>>
where
    K: Key,
{
    fn execute(&mut self, meta: Arc<index::Writer>, writer: &dyn object::Writer) {
        todo!()
    }
}

// impl<V: Key> IndexField for Set<V> {
//     type Item = V;

//     async fn serialize(&self, mw: &mut impl FieldWriter) {
//         for f in self.iter() {
//             mw.write_next(f.key());
//         }
//     }

//     async fn deserialize(&self, mw: &mut impl FieldReader<'_, Self::Item>) {
//         while let Ok(item) = mw.read_next() {
//             self.insert(item);
//         }
//     }
// }
