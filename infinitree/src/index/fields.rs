use crate::{index, object};
use serde::{de::DeserializeOwned, Serialize};
use std::{cmp::Eq, hash::Hash, sync::Arc};

pub trait Value: Serialize + DeserializeOwned + Send + Sync {}
pub trait Key: Serialize + DeserializeOwned + Eq + Hash + Send + Sync {}

impl<T> Value for T where T: Serialize + DeserializeOwned + Send + Sync {}
impl<T> Key for T where T: Serialize + DeserializeOwned + Eq + Hash + Send + Sync {}

mod map;
pub use map::Map;

mod set;
pub use set::Set;

pub trait Store {
    /// Use a specified [`crate::object::Writer`] to execute the Store request
    ///
    /// Note that the provided default implementation leaves this as a
    /// no-op.
    ///
    /// In general, if your Strategy is using an `object::Writer`, you
    /// will want to override this implementation.
    fn execute(&mut self, meta: Arc<index::Writer>, writer: &dyn object::Writer);
}

pub enum QueryAction {
    Take,
    Skip,
    Abort,
}

pub trait Query<I> {
    fn execute(&mut self, predicate: impl Fn(I) -> QueryAction);
}

pub trait Load {
    fn execute(&mut self);
}

#[non_exhaustive]
pub struct Access<T> {
    pub name: String,
    pub strategy: T,
}

impl<T> Access<T> {
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

pub trait Strategy<T: Send>: Send {
    fn for_field(field: &T) -> Self
    where
        Self: Sized;
}

// SparseField
// start shere
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
// LocalField
// start shere
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
