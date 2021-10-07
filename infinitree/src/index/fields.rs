//! Traits and implementations for working with index members
//!
//! There are 3 ways to interact with an index field:
//!
//!  - [`Store`]: Store the field into the index.
//!  - [`Query`]: Query the field and load selected values into memory.
//!  - [`Load`]: Load all contents of the field into memory.
//!
//! To implement how a field is actually stored in the index, we
//! define an access [`Strategy`]. Currently 2 access strategies are
//! implemented in Infinitree, but the storage system is extensible.
//!
//!  - [`SparseField`]: Store the key in the index, but the value in
//!  the object store
//!  - [`LocalField`]: Store both the key and the value in the index.
//!
//! To learn more about index internals, see the module documentation
//! in the [`index`](super) module.

use super::{reader, writer, FieldReader, FieldWriter, TransactionList};
use crate::object;
use serde::{de::DeserializeOwned, Serialize};
use std::{cmp::Eq, hash::Hash, sync::Arc};

/// A marker trait for values that can be serialized and used as a
/// value for an index field.
///
/// You should generally not implement this trait as a blanket
/// implementation will cover all types that conform.
pub trait Value: Serialize + DeserializeOwned + Send + Sync {}

/// A marker trait for value that can be used as a key in an index.
///
/// You should generally not implement this trait as a blanket
/// implementation will cover all types that conform.
pub trait Key: Serialize + DeserializeOwned + Eq + Hash + Send + Sync {}

impl<T> Value for T where T: Serialize + DeserializeOwned + Send + Sync {}
impl<T> Key for T where T: Serialize + DeserializeOwned + Eq + Hash + Send + Sync {}

mod map;
pub use map::Map;

// mod set;
// pub use set::Set;

mod query;
pub use query::*;

mod serialized;
pub use serialized::Serialized;

mod versioned;
pub use versioned::VersionedMap;

pub type List<T> = Arc<parking_lot::RwLock<Vec<T>>>;

/// Store data into the index.
///
/// This trait is usually implemented on a type that also implements
/// [`Strategy`], and _not_ on the field directly.
pub trait Store {
    /// Store the contents of the field into the index. The field
    /// itself needs to track whether this should be a complete
    /// rewrite or an upsert.
    ///
    /// The `transaction` parameter is provided for strategies to
    /// store values in the index, while the `object` is to store
    /// values in the object pool.
    ///
    /// Typically, the [`ChunkPointer`][crate::ChunkPointer] values returned by `object`
    /// should be stored in the index.
    fn execute(&mut self, transaction: &mut writer::Transaction, object: &mut dyn object::Writer);
}

/// Load all data from the index field into memory.
///
/// This trait is usually implemented on a type that also implements
/// [`Strategy`], and _not_ on the field directly.
///
/// In addition, `Load` has a blanket implementation for all types
/// that implement [`Query`], so very likely you never have to
/// manually implement this yourself.
pub trait Load {
    /// Execute a load action.
    ///
    /// The `index` and `object` readers are provided to interact with
    /// the indexes and the object pool, respectively.
    ///
    /// `transaction_list` can contain any list of transactions that
    /// this loader should restore into memory.
    ///
    /// Note that this is decidedly not a type safe way to interact
    /// with a collection, and therefore it is recommended that
    /// `transaction_list` is prepared and sanitized for the field
    /// that's being restored.
    fn load(
        &mut self,
        index: &reader::Reader,
        object: &mut dyn object::Reader,
        transaction_list: TransactionList,
    );
}

impl<K, T> Load for T
where
    T: Select<Key = K>,
{
    #[inline(always)]
    fn load(
        &mut self,
        reader: &reader::Reader,
        object: &mut dyn object::Reader,
        transaction_list: TransactionList,
    ) {
        Select::select(self, reader, object, transaction_list, |_| {
            QueryAction::Take
        })
    }
}

/// Load data into memory where a predicate indicates it's needed
///
/// This trait should be implemented on a type that also implements
/// [`Strategy`], and _not_ on the field directly.
pub trait Select {
    /// The key that the predicate will use to decide whether to pull
    /// more data into memory.
    type Key;

    /// Load items into memory based on a predicate
    ///
    /// The `index` and `object` readers are provided to interact with
    /// the indexes and the object pool, respectively.
    ///
    /// `transaction_list` can contain any list of transactions that
    /// this loader should restore into memory.
    ///
    /// Note that this is decidedly not a type safe way to interact
    /// with a collection, and therefore it is recommended that
    /// `transaction_list` is prepared and sanitized for the field
    /// that's being restored.
    fn select(
        &mut self,
        index: &reader::Reader,
        object: &mut dyn object::Reader,
        transaction_list: TransactionList,
        predicate: impl Fn(&Self::Key) -> QueryAction,
    );
}

/// Query an index field, but do not automatically load it into memory
///
/// To allow lazily loading data from e.g. a [`SparseField`] when
/// relevant, a predicate is taken that controls the iterator.
///
/// This trait should be implemented on a type that also implements
/// [`Strategy`], and _not_ on the field directly.
pub trait Collection {
    /// Use this resolving strategy to load the collection.
    ///
    /// Typically this will be one of two types:
    ///
    ///  * `FullHistory` if a collection requires
    ///     crawling the full transaction history for an accurate
    ///     representation after loading.
    ///  * `LatestOnly` if the collection is not versioned and
    ///     therefore there's no need to resolve the full the
    ///     transaction list.
    type TransactionResolver: TransactionResolver;

    /// The key that the predicate will use to decide whether to pull
    /// more data into memory.
    type Key;

    /// The serialized record format. This type will typically
    /// implement [`serde::Serialize`]
    type Serialized: DeserializeOwned;

    /// This is equivalent to `Iterator::Item`, and should contain a
    /// full record that can be inserted into the in-memory store.
    type Item;

    /// This function is called when initializing an iterator. It will
    /// typically read one-off book keeping information from the
    /// header of the field transaction.
    fn load_head(
        &mut self,
        _transaction: &mut reader::Transaction,
        _object: &mut dyn object::Reader,
    ) {
    }

    /// Get the key based on the deserialized data. You want this to
    /// be a reference that's easy to derive from the serialized data.
    fn key(from: &Self::Serialized) -> &Self::Key;

    /// Load the full record, and return it
    fn load(from: Self::Serialized, object: &mut dyn object::Reader) -> Self::Item;

    /// Store the deserialized record in the collection
    fn insert(&mut self, record: Self::Item);
}

pub trait TransactionResolver {
    fn resolve<'r, R: 'r + AsRef<reader::Reader>>(
        index: R,
        transactions: TransactionList,
    ) -> Box<dyn Iterator<Item = reader::Transaction> + 'r>;
}

pub struct FullHistory;
pub struct FirstOnly;

#[inline(always)]
fn full_history<'r>(
    index: impl AsRef<reader::Reader> + 'r,
    transactions: TransactionList,
) -> impl Iterator<Item = reader::Transaction> + 'r {
    transactions
        .into_iter()
        .map(move |(_gen, field, objectid)| index.as_ref().transaction(field, &objectid).unwrap())
}

impl TransactionResolver for FullHistory {
    #[inline(always)]
    fn resolve<'r, R: 'r + AsRef<reader::Reader>>(
        index: R,
        transactions: TransactionList,
    ) -> Box<dyn Iterator<Item = reader::Transaction> + 'r> {
        Box::new(full_history(index, transactions))
    }
}

impl TransactionResolver for FirstOnly {
    #[inline(always)]
    fn resolve<'r, R: 'r + AsRef<reader::Reader>>(
        index: R,
        transactions: TransactionList,
    ) -> Box<dyn Iterator<Item = reader::Transaction> + 'r> {
        Box::new(full_history(index, transactions).take(1))
    }
}

/// A wrapper to allow working with trait objects and `impl Trait`
/// types when accessing the index field.
#[non_exhaustive]
#[derive(Clone)]
pub struct Access<T> {
    /// The stringy name of the field that's being accessed. This MUST
    /// be unique within the index.
    pub name: String,

    /// The strategy for the given access that's to be executed.
    pub strategy: T,
}

impl<T> Access<T> {
    /// Create a new wrapper that binds a stringy field name to an
    /// access strategy
    #[inline(always)]
    pub fn new(name: impl AsRef<str>, strategy: T) -> Self {
        Access {
            name: name.as_ref().to_string(),
            strategy,
        }
    }
}

impl<T: Store + 'static> From<Access<Box<T>>> for Access<Box<dyn Store>> {
    #[inline(always)]
    fn from(a: Access<Box<T>>) -> Self {
        Access {
            name: a.name,
            strategy: a.strategy as Box<dyn Store>,
        }
    }
}

impl<T: Load + 'static> From<Access<Box<T>>> for Access<Box<dyn Load>> {
    #[inline(always)]
    fn from(a: Access<Box<T>>) -> Self {
        Access {
            name: a.name,
            strategy: a.strategy as Box<dyn Load>,
        }
    }
}

/// Allows decoupling a storage strategy for index fields from the
/// in-memory representation.
pub trait Strategy<T: Send + Sync>: Send + Sync {
    /// Instantiate a new `Strategy`.
    fn for_field(field: &T) -> Self
    where
        Self: Sized;
}

/// A strategy that stores values in the object pool, while keeping
/// keys in the index
pub struct SparseField<Field> {
    field: Field,
}

impl<T: Send + Sync + Clone> Strategy<T> for SparseField<T> {
    #[inline(always)]
    fn for_field(field: &'_ T) -> Self {
        SparseField {
            field: field.clone(),
        }
    }
}

/// A strategy that stores both keys and values in the index
pub struct LocalField<Field> {
    field: Field,
}

impl<T: Send + Sync + Clone> Strategy<T> for LocalField<T> {
    #[inline(always)]
    fn for_field(field: &T) -> Self {
        LocalField {
            field: field.clone(),
        }
    }
}

impl<'iter, T> Select for T
where
    T: Collection,
{
    type Key = T::Key;

    fn select(
        &mut self,
        index: &reader::Reader,
        object: &mut dyn object::Reader,
        transaction_list: TransactionList,
        predicate: impl Fn(&Self::Key) -> QueryAction,
    ) {
        let predicate = Arc::new(predicate);
        for transaction in T::TransactionResolver::resolve(index, transaction_list) {
            let iter = QueryIterator::new(transaction, object, predicate.clone(), self);
            for item in iter {
                self.insert(item);
            }
        }
    }
}

impl<T> Store for LocalField<List<T>>
where
    T: Value,
{
    fn execute(&mut self, transaction: &mut writer::Transaction, _object: &mut dyn object::Writer) {
        for v in self.field.read().iter() {
            transaction.write_next(v);
        }
    }
}

impl<T> Collection for LocalField<List<T>>
where
    T: Value + Clone,
{
    type TransactionResolver = FirstOnly;

    type Key = T;

    type Serialized = T;

    type Item = T;

    fn key(from: &Self::Serialized) -> &Self::Key {
        from
    }

    fn load(from: Self::Serialized, _object: &mut dyn object::Reader) -> Self::Item {
        from
    }

    fn insert(&mut self, record: Self::Item) {
        self.field.write().push(record);
    }
}
