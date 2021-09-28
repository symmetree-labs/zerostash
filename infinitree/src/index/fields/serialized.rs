use super::{FirstOnly, Load, LocalField, Store, TransactionResolver};
use crate::{
    index::{reader, writer, FieldReader, FieldWriter},
    object,
};
use serde::{de::DeserializeOwned, Serialize};
use std::ops::{Deref, DerefMut};

/// A wrapper type that allows using any type that's serializable
/// using serde to be used as a member of the index.
///
/// This implementation is super simplistic, and will not optimize for
/// best performance. If you want something fancy, you are very likely
/// to want to implement your own serialization.
pub struct Serialized<T> {
    inner: T,
}

impl<T> From<T> for Serialized<T> {
    fn from(inner: T) -> Self {
        Self { inner }
    }
}

impl<T: Default> Default for Serialized<T> {
    fn default() -> Self {
        Self {
            inner: T::default(),
        }
    }
}

impl<T: Clone> Clone for Serialized<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<T> Deref for Serialized<T> {
    type Target = T;

    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<T> DerefMut for Serialized<T> {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl<T> Store for LocalField<Serialized<T>>
where
    T: Serialize + Sync,
{
    #[inline(always)]
    fn execute(&mut self, mut transaction: writer::Transaction, _object: &mut dyn object::Writer) {
        transaction.write_next(&*self.field);
    }
}

impl<T> Load for LocalField<Serialized<T>>
where
    T: DeserializeOwned,
{
    fn load(
        &mut self,
        index: &mut reader::Reader,
        object: &mut dyn object::Reader,
        transaction_list: crate::index::TransactionList,
    ) {
        for mut transaction in FirstOnly::resolve(index, transaction_list) {
            *self.field = transaction.read_next().unwrap();
        }
    }
}
