use super::{FirstOnly, Load, LocalField, Store, TransactionResolver};
use crate::{
    index::{reader, writer, FieldReader, FieldWriter},
    object,
};
use parking_lot::RwLock;
use serde::{de::DeserializeOwned, Serialize};
use std::sync::Arc;

/// A wrapper type that allows using any type that's serializable
/// using serde to be used as a member of the index.
///
/// This implementation is super simplistic, and will not optimize for
/// best performance. If you want something fancy, you are very likely
/// to want to implement your own serialization.
#[derive(Default, Clone)]
pub struct Serialized<T>(Arc<RwLock<T>>);

impl<T> Serialized<T> {
    pub fn read(&self) -> parking_lot::lock_api::RwLockReadGuard<parking_lot::RawRwLock, T> {
        self.0.read()
    }

    pub fn write(&self) -> parking_lot::lock_api::RwLockWriteGuard<parking_lot::RawRwLock, T> {
        self.0.write()
    }
}

impl<T> Store for LocalField<Serialized<T>>
where
    T: Serialize + Sync,
{
    #[inline(always)]
    fn execute(&mut self, transaction: &mut writer::Transaction, _object: &mut dyn object::Writer) {
        transaction.write_next(&*self.field.read());
    }
}

impl<T> Load for LocalField<Serialized<T>>
where
    T: DeserializeOwned,
{
    fn load(
        &mut self,
        index: &reader::Reader,
        _object: &mut dyn object::Reader,
        transaction_list: crate::index::TransactionList,
    ) {
        for mut transaction in FirstOnly::resolve(index, transaction_list) {
            *self.field.write() = transaction.read_next().unwrap();
        }
    }
}
