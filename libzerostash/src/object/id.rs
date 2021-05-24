use crate::crypto::{Digest, Random};

use itertools::Itertools;

use std::string::ToString;

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

impl ToString for ObjectId {
    #[inline(always)]
    fn to_string(&self) -> String {
        format!("{:02x}", self.0.as_ref().iter().format(""))
    }
}
