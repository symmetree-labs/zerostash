use crate::backends::Backend;
use crate::compress::{self, STREAM_BLOCK_SIZE};
use crate::crypto::{CryptoProvider, IndexKey};
use crate::index::IndexField;
use crate::meta::{
    Encoder, Field, FieldOffset, FieldWriter, MetaObjectHeader, ObjectIndex, HEADER_SIZE,
};
use crate::object::{ObjectId, WriteObject};

use async_trait::async_trait;
use serde::Serialize;
use serde_cbor::ser::to_vec as serialize_to_vec;

use std::collections::HashMap;
use std::io::{self, Seek, SeekFrom, Write};
use std::sync::Arc;

pub type WriteError = io::Error;
pub type Result<T> = std::result::Result<T, WriteError>;

pub struct Writer {
    objects: ObjectIndex,
    offsets: Vec<FieldOffset>,
    encoder: WriteState,
    current_field: Option<Field>,
    backend: Arc<dyn Backend>,
    crypto: IndexKey,
}

#[async_trait]
impl FieldWriter for Writer {
    async fn write_next(&mut self, obj: impl Serialize + Send + 'async_trait) {
        let writer = self.encoder.writer().unwrap();
        let capacity = writer.capacity();
        let position = writer.position();

        let record = serialize_to_vec(&obj).unwrap();

        if capacity - position < STREAM_BLOCK_SIZE {
            self.seal_and_store().await;
        }

        if record.len() + position > capacity - 64 {
            self.seal_and_store().await;
        }

        self.encoder.start().unwrap().write_all(&record).unwrap();
    }
}

impl Writer {
    pub fn new(
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
            objects: HashMap::new(),
            current_field: None,
            backend,
            crypto,
        })
    }

    pub fn objects(&self) -> &ObjectIndex {
        &self.objects
    }

    pub async fn write_field<F: IndexField>(&mut self, name: &str, obj: &F) {
        // book keeping
        self.offsets.push(FieldOffset(
            self.encoder.writer().unwrap().position() as u32,
            name.into(),
        ));

        self.objects
            .entry(name.into())
            .or_default()
            .insert(*self.encoder.writer().unwrap().id());

        self.encoder.start().unwrap();

        // clean up
        self.current_field = Some(name.into());
        obj.serialize(self).await;
        self.current_field = None;

        // skip to next multiple of STREAM_BLOCK_SIZE
        let object = self.encoder.writer().unwrap();
        let skip = STREAM_BLOCK_SIZE - (object.position() - HEADER_SIZE) % STREAM_BLOCK_SIZE;

        if skip + object.position() < object.capacity() {
            let mut object = self.encoder.finish().unwrap();
            object.seek(SeekFrom::Current(skip as i64)).unwrap();
            self.encoder = WriteState::Parked(object);
        } else {
            self.seal_and_store().await;
        }
    }

    pub async fn seal_and_store(&mut self) {
        let mut object = self.encoder.finish().unwrap();
        let end = object.position();

        // fill the end of the object with random & other stuff
        object.finalize(&self.crypto);
        let next_object_id = ObjectId::new(&self.crypto);

        let object_header = MetaObjectHeader::new(
            self.current_field.clone().map(|_| next_object_id),
            &self.offsets,
            end,
        );
        let header_bytes = serialize_to_vec(&object_header).expect("failed to write header");

        // ok, this is pretty rough, but it also shouldn't happen, so yolo
        assert!(header_bytes.len() < HEADER_SIZE);
        object.write_head(&header_bytes);

        // encrypt & store
        self.crypto.encrypt_object(&mut object);
        self.backend.write_object(&object).unwrap();

        // track which objects are holding what kind of data
        for fo in self.offsets.drain(..) {
            self.objects
                .entry(fo.as_field())
                .or_default()
                .insert(*object.id());
        }

        // start cleaning up and bookkeeping
        object.set_id(next_object_id);

        // re-initialize the object
        object.clear();
        object.seek(SeekFrom::Start(HEADER_SIZE as u64)).unwrap();
        self.encoder = WriteState::Parked(object);

        // make sure we register the currently written field in the new object
        if let Some(f) = &self.current_field {
            self.offsets
                .push(FieldOffset::new(HEADER_SIZE as u32, f.to_owned()));
        }
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
