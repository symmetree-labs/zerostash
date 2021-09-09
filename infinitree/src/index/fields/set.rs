use super::{Key, LocalField, QueryAction, Select, Store};
use crate::{
    index::{self, FieldReader, FieldWriter},
    object,
};
use scc::HashMap;
use std::sync::Arc;

pub type Set<K> = Arc<HashMap<K, ()>>;

impl<'index, K> Store for LocalField<Set<K>>
where
    K: Key,
{
    fn execute(
        &mut self,
        mut transaction: index::writer::Transaction,
        _writer: &mut dyn object::Writer,
    ) {
        for f in self.field.iter() {
            transaction.write_next(f.key());
        }
    }
}

impl<'index, K> Select for LocalField<Set<K>>
where
    K: Key,
{
    type Key = K;

    fn execute(
        &mut self,
        mut transaction: index::reader::Transaction,
        _object: &mut dyn object::Reader,
        predicate: impl Fn(&K) -> QueryAction,
    ) {
        while let Ok(item) = transaction.read_next() {
            use QueryAction::*;

            match (predicate)(&item) {
                Take => {
                    self.field.insert(item);
                }
                Skip => (),
                Abort => break,
            }
        }
    }
}
