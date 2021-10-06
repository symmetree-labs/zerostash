use super::{Reader, Result, Writer};
use crate::ChunkPointer;
use serde::{de::DeserializeOwned, Serialize};

#[derive(Serialize, Deserialize)]
pub struct SizedPointer {
    chunk: ChunkPointer,
    data_size: usize,
}

pub fn write<'writer, T: Serialize, W: 'writer + Writer + ?Sized>(
    writer: &mut W,
    serialize: impl Fn(T) -> Result<Vec<u8>>,
    obj: T,
) -> Result<SizedPointer> {
    let d = (serialize)(obj)?;
    let data_size = d.len();
    let hash = crate::crypto::secure_hash(&d);
    let chunk = writer.write_chunk(&hash, &d)?;

    Ok(SizedPointer { chunk, data_size })
}

pub fn read<T: DeserializeOwned, R: Reader + ?Sized>(
    reader: &mut R,
    deserialize: impl Fn(&[u8]) -> Result<T>,
    pointer: SizedPointer,
) -> Result<T> {
    let mut serialized = vec![0; pointer.data_size];
    reader.read_chunk(&pointer.chunk, &mut serialized)?;

    deserialize(&serialized)
}
