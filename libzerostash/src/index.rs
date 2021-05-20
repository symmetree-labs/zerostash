use crate::{
    compress,
    object::{ObjectId, WriteObject},
};
use std::{
    collections::{HashMap, HashSet},
    error::Error,
    io::Cursor,
};

use async_trait::async_trait;
use serde::{de::DeserializeOwned, Serialize};

mod fields;
mod header;
mod reader;
mod writer;

pub use fields::*;
pub use header::*;
pub use reader::{ReadError, Reader};
pub use writer::{WriteError, Writer};

type Encoder = compress::Encoder<WriteObject>;
type Decoder<'b> =
    serde_cbor::Deserializer<serde_cbor::de::IoRead<compress::Decoder<Cursor<&'b [u8]>>>>;
pub type ObjectIndex = HashMap<Field, HashSet<ObjectId>>;

#[async_trait]
pub trait Index {
    async fn read_fields(
        &mut self,
        metareader: reader::Reader,
        start_object: ObjectId,
    ) -> Result<(), Box<dyn std::error::Error>>;

    async fn write_fields(
        &mut self,
        metareader: &mut writer::Writer,
    ) -> Result<(), Box<dyn std::error::Error>>;
}

#[async_trait]
pub trait FieldWriter: Send {
    async fn write_next(&mut self, obj: impl Serialize + Send + 'async_trait);
}

#[async_trait]
pub trait FieldReader<T>: Send {
    async fn read_next(&mut self) -> Result<T, Box<dyn Error>>;
}

#[async_trait]
impl<'b, T> FieldReader<T> for Decoder<'b>
where
    T: DeserializeOwned,
{
    async fn read_next(&mut self) -> Result<T, Box<dyn Error>> {
        Ok(T::deserialize(self)?)
    }
}
