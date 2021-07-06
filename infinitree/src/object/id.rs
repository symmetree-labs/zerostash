use crate::crypto::{Digest, Random};

pub use hex::FromHexError;

use std::{convert::TryFrom, string::ToString};

#[derive(Debug, Default, Clone, Copy, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectId(Digest);

impl ObjectId {
    #[inline(always)]
    pub fn new(random: &impl Random) -> ObjectId {
        let mut id = ObjectId::default();
        id.reset(random);
        id
    }

    pub fn from_bytes(bytes: impl AsRef<[u8]>) -> ObjectId {
        let mut id = ObjectId::default();
        id.0.copy_from_slice(bytes.as_ref());

        id
    }

    #[inline(always)]
    pub fn reset(&mut self, random: &impl Random) {
        random.fill(&mut self.0);
    }
}

impl AsRef<[u8]> for ObjectId {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl TryFrom<&str> for ObjectId {
    type Error = FromHexError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        hex::decode(value).map(Self::from_bytes)
    }
}

impl ToString for ObjectId {
    #[inline(always)]
    fn to_string(&self) -> String {
        hex::encode(self.0.as_ref())
    }
}
