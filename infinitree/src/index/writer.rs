#![allow(clippy::large_enum_variant)]
use super::{Encoder, FieldOffset, FieldWriter, Header, HEADER_SIZE};
use crate::{
    backends::Backend,
    compress,
    crypto::{CryptoProvider, IndexKey},
    object::{ObjectId, WriteObject},
    serialize_to_vec,
};

use serde::Serialize;

use std::{
    io::{self, Seek, SeekFrom, Write},
    mem,
    sync::Arc,
};

pub struct Transaction<'writer> {
    writer: &'writer mut Writer,
    start_object: ObjectId,
}

impl<'writer> Transaction<'writer> {
    pub(crate) fn finish(self) -> ObjectId {
        self.start_object
    }
}

impl<'writer> FieldWriter for Transaction<'writer> {
    fn write_next(&mut self, obj: impl Serialize + Send) {
        let writer = self.writer.encoder.writer().unwrap();
        let capacity = writer.capacity();
        let position = writer.position();

        let record = serialize_to_vec(&obj).unwrap();

        if capacity - position < compress::FRAME_BLOCK_SIZE.get_size() {
            self.writer.seal_and_store();
        }

        if record.len() + position > capacity - 64 {
            self.writer.seal_and_store();
        }

        self.writer
            .encoder
            .start()
            .unwrap()
            .write_all(&record)
            .unwrap();
    }
}

impl<'writer> Drop for Transaction<'writer> {
    fn drop(&mut self) {
        eprintln!(
            "dropping transaction: {} {}",
            self.writer.encoder.writer().unwrap().id().to_string(),
            self.writer.encoder.writer().unwrap().position()
        );

        // unwrap here can't fail because a transaction implies
        // `current_field = Some(_)`
        self.writer
            .offsets
            .push(mem::take(&mut self.writer.current_field).unwrap());

        let object = self.writer.encoder.writer().unwrap();
        let block_size = compress::FRAME_BLOCK_SIZE.get_size();
        let skip = block_size - (object.position() - HEADER_SIZE) % block_size;

        if skip + object.position() < object.capacity() {
            let mut object = self.writer.encoder.finish().unwrap();
            object.seek(SeekFrom::Current(skip as i64)).unwrap();
            self.writer.encoder = WriteState::Parked(object);
        } else {
            self.writer.seal_and_store();
        }
    }
}

pub(crate) type Result<T> = std::result::Result<T, io::Error>;
pub(crate) struct Writer {
    offsets: Vec<FieldOffset>,
    encoder: WriteState,
    current_field: Option<FieldOffset>,

    backend: Arc<dyn Backend>,
    crypto: IndexKey,
}

impl<'writer> Writer {
    pub(crate) fn new(
        root_object_id: ObjectId,
        backend: Arc<dyn Backend>,
        crypto: IndexKey,
    ) -> Result<Self> {
        let mut object = WriteObject::default();
        object.reserve_tag();
        object.set_id(root_object_id);
        object.seek(SeekFrom::Start(HEADER_SIZE as u64))?;

        Ok(Writer {
            encoder: WriteState::Parked(object),
            offsets: vec![],
            current_field: None,
            backend,
            crypto,
        })
    }

    pub(crate) fn transaction(&'writer mut self, name: &str) -> Transaction<'writer> {
        let oid = *self.encoder.writer().unwrap().id();

        let start_object = oid;

        self.encoder.start().unwrap();

        self.current_field = Some(FieldOffset {
            offset: self.encoder.writer().unwrap().position() as u32,
            name: name.into(),
            next: None,
        });
        eprintln!(
            "starting transaction: {} {:?}",
            oid.to_string(),
            self.current_field
        );

        Transaction {
            writer: self,
            start_object,
        }
    }

    pub(crate) fn seal_and_store(&mut self) {
        let mut object = self.encoder.finish().unwrap();
        let next_object_id = ObjectId::new(&self.crypto);
        let end = object.position();

        if let Some(ref mut f) = &mut self.current_field {
            let mut field_ref = f.clone();
            field_ref.next = Some(next_object_id);

            self.offsets.push(field_ref);

            // this is for the `next` object to be picked up
            f.offset = HEADER_SIZE as u32;
        }

        eprintln!(
            "sealing {}; size: {}, {:?}",
            object.id().to_string(),
            end,
            self.offsets
        );
        let object_header = Header::new(&self.offsets, end);
        let header_bytes = serialize_to_vec(&object_header).expect("failed to write header");

        // ok, this is pretty rough, but it also shouldn't happen, so yolo
        assert!(header_bytes.len() < HEADER_SIZE);
        object.write_head(&header_bytes);

        // fill the end of the object with random & other stuff
        object.finalize(&self.crypto);

        // encrypt & store
        self.crypto.encrypt_object(&mut object);
        self.backend.write_object(&object).unwrap();

        // start cleaning up and bookkeeping
        object.set_id(next_object_id);

        // re-initialize the object
        object.clear();
        object.seek(SeekFrom::Start(HEADER_SIZE as u64)).unwrap();

        self.encoder = WriteState::Parked(object);
        self.offsets.clear();
    }
}

enum WriteState {
    Idle,
    Parked(WriteObject),
    Encoding(Encoder),
}

impl WriteState {
    fn start(&mut self) -> Result<&mut Self> {
        use WriteState::*;

        match self {
            Idle => unreachable!(),
            Parked(_) => {
                let mut tmp = WriteState::Idle;
                std::mem::swap(&mut tmp, self);

                let encoder = match tmp {
                    Parked(w) => compress::stream(w),
                    _ => unreachable!(),
                };

                let _ = std::mem::replace(self, WriteState::Encoding(encoder));
                Ok(self)
            }
            Encoding(_) => Ok(self),
        }
    }

    fn finish(&mut self) -> Result<WriteObject> {
        use WriteState::*;

        let mut encoder = WriteState::Idle;
        std::mem::swap(self, &mut encoder);

        match encoder {
            Idle => unreachable!(),
            Parked(w) => Ok(w),
            Encoding(e) => Ok(e.finish()?),
        }
    }

    fn writer(&self) -> Result<&WriteObject> {
        use WriteState::*;
        match self {
            Idle => unreachable!(),
            Parked(w) => Ok(w),
            Encoding(e) => Ok(e.get_ref()),
        }
    }
}

impl Write for WriteState {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        use io::{Error, ErrorKind};
        use WriteState::*;

        match self {
            Idle => Err(Error::new(ErrorKind::Other, "Uninitialized")),
            Parked(_) => Err(Error::new(ErrorKind::Other, "Inactive")),
            Encoding(e) => e.write(buf),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
