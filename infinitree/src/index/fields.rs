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

use super::{reader, writer, FieldReader, FieldWriter};
use crate::object;
use serde::{de::DeserializeOwned, Serialize};
use std::{
    cmp::Eq,
    hash::Hash,
    ops::{Deref, DerefMut},
};

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

mod set;
pub use set::Set;

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
    fn execute(&mut self, transaction: writer::Transaction, object: &mut dyn object::Writer);
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
    /// The `transaction` parameter is provided for strategies to
    /// load values from the index, while the `object` is to load
    /// values from the object pool.
    fn execute(&mut self, transaction: reader::Transaction, object: &mut dyn object::Reader);
}

impl<K, T> Load for T
where
    T: Query<Key = K>,
{
    fn execute(&mut self, transaction: reader::Transaction, object: &mut dyn object::Reader) {
        Query::execute(self, transaction, object, |_| QueryAction::Take)
    }
}

/// Load data from the index field into memory based on a predicate.
///
/// This trait is usually implemented on a type that also implements
/// [`Strategy`], and _not_ on the field directly.
pub trait Query {
    /// The key that the predicate will use to decide whether to pull
    /// more data into memory.
    type Key;

    /// Execute a load action.
    ///
    /// The `transaction` parameter is provided for strategies to
    /// load values from the index, while the `object` is to load
    /// values from the object pool.
    fn execute(
        &mut self,
        transaction: reader::Transaction,
        object: &mut dyn object::Reader,
        predicate: impl Fn(&Self::Key) -> QueryAction,
    );
}

/// Result of a query predicate.
pub enum QueryAction {
    /// Pull the current value into memory.
    Take,
    /// Skip the current value and deserialize the next one.
    Skip,
    /// Abort the query and _don't_ pull the current value to memory.
    Abort,
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
    pub fn new(name: impl AsRef<str>, strategy: T) -> Self {
        Access {
            name: name.as_ref().to_string(),
            strategy,
        }
    }
}

impl<T: Store + 'static> From<Access<Box<T>>> for Access<Box<dyn Store>> {
    fn from(a: Access<Box<T>>) -> Self {
        Access {
            name: a.name,
            strategy: a.strategy as Box<dyn Store>,
        }
    }
}

impl<T: Load + 'static> From<Access<Box<T>>> for Access<Box<dyn Load>> {
    fn from(a: Access<Box<T>>) -> Self {
        Access {
            name: a.name,
            strategy: a.strategy as Box<dyn Load>,
        }
    }
}

/// Allows decoupling a storage strategy for index fields from the
/// in-memory representation.
pub trait Strategy<T: Send>: Send {
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

impl<T: Send + Clone> Strategy<T> for SparseField<T> {
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

impl<T: Send + Clone> Strategy<T> for LocalField<T> {
    fn for_field(field: &T) -> Self {
        LocalField {
            field: field.clone(),
        }
    }
}

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

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<T> DerefMut for Serialized<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl<T> Store for LocalField<Serialized<T>>
where
    T: Serialize + Sync,
{
    fn execute(&mut self, mut transaction: writer::Transaction, _object: &mut dyn object::Writer) {
        transaction.write_next(&*self.field);
    }
}

impl<T> Load for LocalField<Serialized<T>>
where
    T: DeserializeOwned,
{
    fn execute(&mut self, mut transaction: reader::Transaction, _object: &mut dyn object::Reader) {
        *self.field = transaction.read_next().unwrap();
    }
}
