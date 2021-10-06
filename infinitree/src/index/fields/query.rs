use super::{reader, Collection, FieldReader};
use crate::object;
use std::{marker::PhantomData, sync::Arc};

/// Result of a query predicate.
pub enum QueryAction {
    /// Pull the current value into memory.
    Take,
    /// Skip the current value and deserialize the next one.
    Skip,
    /// Abort the query and _don't_ pull the current value to memory.
    Abort,
}

pub(crate) struct QueryIteratorOwned<T, F, R> {
    transaction: reader::Transaction,
    object: R,
    predicate: Arc<F>,
    _fieldtype: PhantomData<T>,
}

impl<T, K, R, F> QueryIteratorOwned<T, F, R>
where
    T: Collection<Key = K>,
    F: Fn(&K) -> QueryAction,
    R: object::Reader,
{
    pub fn new(
        mut transaction: reader::Transaction,
        mut object: R,
        predicate: Arc<F>,
        field: &mut T,
    ) -> Self {
        field.load_head(&mut transaction, &mut object);
        Self {
            transaction,
            object,
            predicate,
            _fieldtype: PhantomData,
        }
    }
}

impl<T, K, R, F> Iterator for QueryIteratorOwned<T, F, R>
where
    T: Collection<Key = K>,
    F: Fn(&K) -> QueryAction,
    R: object::Reader,
{
    type Item = <T as Collection>::Item;

    #[inline(always)]
    fn next(&mut self) -> Option<Self::Item> {
        while let Ok(item) = self.transaction.read_next::<T::Serialized>() {
            use QueryAction::*;

            match (self.predicate)(T::key(&item)) {
                Take => return Some(T::load(item, &mut self.object)),
                Skip => continue,
                Abort => return None,
            }
        }

        None
    }
}

pub(crate) struct QueryIterator<'reader, T, F> {
    transaction: reader::Transaction,
    object: &'reader mut dyn object::Reader,
    predicate: Arc<F>,
    _fieldtype: PhantomData<T>,
}

impl<'reader, T, K, F> QueryIterator<'reader, T, F>
where
    T: Collection<Key = K>,
    F: Fn(&K) -> QueryAction,
{
    pub fn new(
        mut transaction: reader::Transaction,
        object: &'reader mut dyn object::Reader,
        predicate: Arc<F>,
        field: &mut T,
    ) -> Self {
        field.load_head(&mut transaction, object);
        Self {
            transaction,
            object,
            predicate,
            _fieldtype: PhantomData,
        }
    }
}

impl<'reader, T, K, F> Iterator for QueryIterator<'reader, T, F>
where
    T: Collection<Key = K>,
    F: Fn(&K) -> QueryAction,
{
    type Item = <T as Collection>::Item;

    #[inline(always)]
    fn next(&mut self) -> Option<Self::Item> {
        while let Ok(item) = self.transaction.read_next::<T::Serialized>() {
            use QueryAction::*;

            match (self.predicate)(T::key(&item)) {
                Take => return Some(T::load(item, self.object)),
                Skip => continue,
                Abort => return None,
            }
        }

        None
    }
}
