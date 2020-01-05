use crate::backends::*;
use crate::compress;
use crate::crypto::CryptoProvider;
use crate::meta::{Field, MetaObjectField, MetaObjectHeader, ObjectIndex};
use crate::objects::{BlockBuffer, Object, ObjectId};

use failure::Error;

use std::borrow::Borrow;
use std::io::Cursor;

pub struct Reader<B, C> {
    inner: Object<BlockBuffer>,
    header: Option<MetaObjectHeader>,
    objects: ObjectIndex,
    backend: B,
    crypto: C,
}

impl<B, C> Reader<B, C>
where
    B: Backend,
    C: CryptoProvider,
{
    pub fn new(backend: B, crypto: C) -> Reader<B, C> {
        Reader {
            inner: Object::default(),
            objects: ObjectIndex::default(),
            header: None,
            backend,
            crypto,
        }
    }

    pub fn open(&mut self, id: &ObjectId) -> Result<MetaObjectHeader, Error> {
        let obj = self.backend.read_object(id)?;

        self.inner.reset_cursor();
        self.inner.set_id(*id);
        self.crypto.decrypt_object_into(&mut self.inner, &obj);

        let mut de = serde_cbor::Deserializer::from_slice(self.inner.as_ref()).into_iter();
        self.header = de.next().ok_or_else(|| format_err!("bad header"))?.ok();

        self.header.clone().ok_or_else(|| format_err!("no header"))
    }

    pub fn read_into(
        &mut self,
        field: impl Borrow<Field>,
        store: &mut impl MetaObjectField,
    ) -> Result<(), Error> {
        let field = field.borrow();

        match self.header {
            None => bail!("no header"),
            Some(ref header) => {
                let frame_start = header
                    .get_offset(&field)
                    .ok_or_else(|| format_err!("no field"))?
                    as usize;

                let buffer: &[u8] = self.inner.as_ref();
                let decompress =
                    compress::destream(Cursor::new(&buffer[frame_start..header.end()]))?;

                let mut reader = serde_cbor::Deserializer::from_reader(decompress);

                store.deserialize(&mut reader);
                Ok(())
            }
        }
    }
}
