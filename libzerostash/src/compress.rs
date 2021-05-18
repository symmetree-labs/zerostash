use lz4_flex::frame::{BlockMode, BlockSize, FrameInfo};
pub use lz4_flex::{
    block::{compress_into, decompress_into, get_maximum_output_size, CompressError},
    frame::{FrameDecoder as Decoder, FrameEncoder as Encoder},
};

use std::io::{Read, Write};

pub const STREAM_BLOCK_SIZE: usize = 64 * 1024;
const LZ4_BLOCK_SIZE: BlockSize = BlockSize::Max64KB;

pub fn stream<W: Write>(w: W) -> Encoder<W> {
    let mut config = FrameInfo::new();

    config.block_size = LZ4_BLOCK_SIZE;
    config.block_mode = BlockMode::Linked;
    config.block_checksums = false;

    Encoder::with_frame_info(config, w)
}

pub fn destream<R: Read>(r: R) -> Decoder<R> {
    Decoder::new(r)
}
