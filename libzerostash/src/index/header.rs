use crate::object::ObjectId;

// Header size max 512b
pub(crate) const HEADER_SIZE: usize = 512;

pub(crate) type Field = String;
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct FieldOffset(pub(crate) u32, pub(crate) Field);

impl From<&FieldOffset> for u32 {
    fn from(fo: &FieldOffset) -> u32 {
        fo.0
    }
}

impl FieldOffset {
    pub(crate) fn new(offs: u32, f: Field) -> Self {
        FieldOffset(offs, f)
    }

    pub(crate) fn as_field(&self) -> Field {
        self.1.to_owned()
    }
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub enum Header {
    V1 {
        next_object: Option<ObjectId>,
        offsets: Vec<FieldOffset>,
        end: usize,
    },
}

impl Header {
    pub(crate) fn new(
        next_object: Option<ObjectId>,
        offsets: impl AsRef<[FieldOffset]>,
        end: usize,
    ) -> Self {
        Header::V1 {
            offsets: offsets.as_ref().to_vec(),
            next_object,
            end,
        }
    }

    pub fn next_object(&self) -> Option<ObjectId> {
        match self {
            Header::V1 {
                ref next_object, ..
            } => *next_object,
        }
    }

    pub fn fields(&self) -> Vec<Field> {
        match self {
            Header::V1 { ref offsets, .. } => offsets.iter().map(FieldOffset::as_field).collect(),
        }
    }

    pub(crate) fn end(&self) -> usize {
        match self {
            Header::V1 { ref end, .. } => *end,
        }
    }

    pub(crate) fn get_offset(&self, field: &str) -> Option<u32> {
        match self {
            Header::V1 { ref offsets, .. } => {
                for fo in offsets.iter() {
                    if fo.as_field() == field {
                        return Some(fo.into());
                    }
                }
                None
            }
        }
    }
}
