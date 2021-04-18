use lz4::block::{compress, decompress, CompressionMode};
use lz4::{BlockMode, BlockSize, ContentChecksum, EncoderBuilder};
pub use lz4::{Decoder, Encoder};

use std::io::{Read, Result, Write};

pub const STREAM_LEVEL: u32 = 1;
pub const BLOCK_LEVEL: i32 = 32;
pub const STREAM_BLOCK_SIZE: usize = 64 * 1024;
const LZ4_BLOCK_SIZE: BlockSize = BlockSize::Max64KB;

pub fn block(buf: &[u8]) -> Result<Vec<u8>> {
    compress(&buf, Some(CompressionMode::FAST(BLOCK_LEVEL)), true)
}

pub fn deblock(buf: &[u8]) -> Result<Vec<u8>> {
    decompress(&buf, None)
}

pub fn stream<W: Write>(w: W) -> Result<Encoder<W>> {
    EncoderBuilder::new()
        .level(STREAM_LEVEL)
        .block_mode(BlockMode::Independent)
        .block_size(LZ4_BLOCK_SIZE)
        .checksum(ContentChecksum::NoChecksum)
        .build(w)
}

pub fn destream<R: Read>(r: R) -> Result<Decoder<R>> {
    Decoder::new(r)
}

pub fn decompress_into(dst: &mut [u8], mut src: &[u8]) -> Result<u32> {
    // Copied and adapted from https://github.com/bozaro/lz4-rs/blob/master/src/block/mod.rs
    use libc::c_char;
    use lz4::liblz4::*;
    use std::io::{Error, ErrorKind};

    let size;

    if src.len() < 4 {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "Source buffer must at least contain size prefix.",
        ));
    }
    size = (src[0] as i32) | (src[1] as i32) << 8 | (src[2] as i32) << 16 | (src[3] as i32) << 24;

    src = &src[4..];

    if size <= 0 {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "Parsed size prefix in buffer must not be negative.",
        ));
    }

    if unsafe { LZ4_compressBound(size) } <= 0 {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "Given size parameter is too big",
        ));
    }

    let dec_bytes = unsafe {
        LZ4_decompress_safe(
            src.as_ptr() as *const c_char,
            dst.as_mut_ptr() as *mut c_char,
            src.len() as i32,
            size,
        )
    };

    if dec_bytes < 0 {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "Decompression failed. Input invalid or too long?",
        ));
    }

    // size is strictly > 0 at this point, so this is safe
    Ok(size as u32)
}
