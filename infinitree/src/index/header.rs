use crate::object::ObjectId;

// Header size max 512b
pub(crate) const HEADER_SIZE: usize = 512;

pub(crate) type Field = String;

/// The offset of a field's LZ4 stream from the beginning of an object
///
/// This type allows listing all fields contained in an index object
/// inside the header.
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct FieldOffset {
    pub(crate) offset: u32,
    pub(crate) name: Field,
    pub(crate) next: Option<ObjectId>,
}

impl From<&FieldOffset> for u32 {
    fn from(fo: &FieldOffset) -> u32 {
        fo.offset
    }
}

impl FieldOffset {
    pub(crate) fn as_field(&self) -> Field {
        self.name.to_owned()
    }
}

/// The header of an index object.
///
/// The structure allows versioning of internal structure, and may be
/// extended in the future.
///
/// There's usually no need to instantiate a `Header` in your
/// application unless you know what you're doing.
///
/// For more information about how headers control
/// serialization/deserialization, please look at the [`index`](super)
/// module's documentation.
#[derive(Clone, Serialize, Deserialize, Debug)]
pub enum Header {
    V1 {
        offsets: Vec<FieldOffset>,
        end: usize,
    },
}

impl Header {
    pub(crate) fn new(offsets: impl AsRef<[FieldOffset]>, end: usize) -> Self {
        Header::V1 {
            offsets: offsets.as_ref().to_vec(),
            end,
        }
    }

    pub fn next_object(&self, field: &str) -> Option<ObjectId> {
        match self {
            Header::V1 { ref offsets, .. } => {
                for fo in offsets.iter() {
                    if fo.as_field() == field {
                        return fo.next;
                    }
                }
                None
            }
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
