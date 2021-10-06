use crate::{
    chunks::RawChunkPointer,
    object::{ObjectId, WriteObject},
};

use blake2b_simd::blake2bp::Params as Blake2;
use getrandom::getrandom;
use ring::aead;
use secrecy::{ExposeSecret, Secret};
use thiserror::Error;
use zeroize::Zeroize;

const CRYPTO_DIGEST_SIZE: usize = 32;
type Nonce = [u8; 12];
type RawKey = Secret<[u8; CRYPTO_DIGEST_SIZE]>;

pub type Digest = [u8; CRYPTO_DIGEST_SIZE];
pub type Tag = [u8; 16];

#[derive(Error, Debug)]
pub enum CryptoError {
    #[error("Key error: {source}")]
    KeyError {
        #[from]
        source: argon2::Error,
    },
}
pub type Result<T> = std::result::Result<T, CryptoError>;

pub struct Key {
    master_key: RawKey,
}

pub trait Random {
    fn fill(&self, buf: &mut [u8]);
}

#[derive(Clone)]
pub struct ObjectOperations {
    key: RawKey,
}

pub type IndexKey = ObjectOperations;
pub type ChunkKey = ObjectOperations;

#[inline]
pub fn secure_hash(content: &[u8]) -> Digest {
    let mut output = Digest::default();

    output.copy_from_slice(
        Blake2::new()
            .hash_length(CRYPTO_DIGEST_SIZE)
            .hash(content)
            .as_bytes(),
    );

    output
}

pub(crate) trait CryptoProvider: Random + Send + Sync + Clone {
    fn encrypt_chunk(&self, object_id: &ObjectId, hash: &Digest, data: &mut [u8]) -> Tag;
    fn encrypt_object(&self, object: &mut WriteObject);

    fn decrypt_chunk<'buf>(
        &self,
        target: &'buf mut [u8],
        source: &[u8],
        source_id: &ObjectId,
        chunk: &RawChunkPointer,
    ) -> &'buf mut [u8];

    fn decrypt_object_into(&self, target: &mut [u8], source: &[u8], source_id: &ObjectId);
}

impl Key {
    pub fn from_credentials(username: impl AsRef<str>, password: impl AsRef<str>) -> Result<Key> {
        derive_argon2(username.as_ref().as_bytes(), password.as_ref().as_bytes())
            .map(|k| Key { master_key: k })
    }

    pub(crate) fn root_object_id(&self) -> Result<ObjectId> {
        derive_subkey(&self.master_key, b"_0s_root")
            .map(|k| ObjectId::from_bytes(k.expose_secret()))
    }

    pub(crate) fn get_meta_key(&self) -> Result<IndexKey> {
        derive_subkey(&self.master_key, b"_0s_meta").map(ObjectOperations::new)
    }

    pub(crate) fn get_object_key(&self) -> Result<ChunkKey> {
        derive_subkey(&self.master_key, b"_0s_obj_").map(ObjectOperations::new)
    }
}

impl ObjectOperations {
    pub fn new(key: RawKey) -> ObjectOperations {
        ObjectOperations { key }
    }
}

impl Random for ObjectOperations {
    #[inline]
    fn fill(&self, buf: &mut [u8]) {
        getrandom(buf).unwrap()
    }
}

impl CryptoProvider for ObjectOperations {
    #[inline]
    fn encrypt_chunk(&self, object_id: &ObjectId, hash: &Digest, data: &mut [u8]) -> Tag {
        let aead = get_aead(derive_chunk_key(&self.key, hash));
        let tag = aead
            .seal_in_place_separate_tag(
                get_chunk_nonce(object_id, data.len() as u32),
                aead::Aad::empty(),
                data,
            )
            .unwrap();

        let mut t = Tag::default();
        t.copy_from_slice(tag.as_ref());
        t
    }

    #[inline]
    fn encrypt_object(&self, object: &mut WriteObject) {
        let aead = get_aead(self.key.clone());

        let tag = aead
            .seal_in_place_separate_tag(
                get_object_nonce(object.id()),
                aead::Aad::empty(),
                object.as_mut(),
            )
            .unwrap();

        object.write_tag(tag.as_ref());
    }

    #[inline]
    fn decrypt_chunk<'buf>(
        &self,
        target: &'buf mut [u8],
        source: &[u8],
        source_id: &ObjectId,
        chunk: &RawChunkPointer,
    ) -> &'buf mut [u8] {
        let size = chunk.size as usize;
        let cyphertext_size = size + chunk.tag.len();

        assert!(target.len() >= cyphertext_size);

        let start = chunk.offs as usize;
        let end = start + size;

        target[..size].copy_from_slice(&source[start..end]);
        target[size..cyphertext_size].copy_from_slice(&chunk.tag);

        let aead = get_aead(derive_chunk_key(&self.key, &chunk.hash));
        aead.open_in_place(
            get_chunk_nonce(source_id, chunk.size),
            aead::Aad::empty(),
            &mut target[..cyphertext_size],
        )
        .unwrap();

        &mut target[..size]
    }

    #[inline]
    fn decrypt_object_into(&self, target: &mut [u8], source: &[u8], source_id: &ObjectId) {
        target.copy_from_slice(source);

        let aead = get_aead(self.key.clone());
        aead.open_in_place(get_object_nonce(source_id), aead::Aad::empty(), target)
            .unwrap();
    }
}

#[inline]
fn get_aead(key: RawKey) -> aead::LessSafeKey {
    let key =
        aead::UnboundKey::new(&aead::CHACHA20_POLY1305, key.expose_secret()).expect("bad key");
    aead::LessSafeKey::new(key)
}

#[inline]
fn derive_chunk_key(key_src: &RawKey, hash: &Digest) -> RawKey {
    let mut key = *key_src.expose_secret();
    for i in 0..key.len() {
        key[i] ^= hash[i];
    }
    Secret::new(key)
}

#[inline]
fn get_object_nonce(object_id: &ObjectId) -> aead::Nonce {
    let mut nonce = Nonce::default();
    let len = nonce.len();

    nonce.copy_from_slice(&object_id.as_ref()[..len]);
    aead::Nonce::assume_unique_for_key(nonce)
}

#[inline]
fn get_chunk_nonce(object_id: &ObjectId, data_size: u32) -> aead::Nonce {
    let mut nonce = Nonce::default();
    let len = nonce.len();
    nonce.copy_from_slice(&object_id.as_ref()[..len]);

    let size = data_size.to_le_bytes();
    for i in 0..size.len() {
        nonce[i] ^= size[i];
    }

    aead::Nonce::assume_unique_for_key(nonce)
}

fn derive_argon2(salt_raw: &[u8], password: &[u8]) -> Result<RawKey> {
    let salt = Blake2::new().hash_length(16).hash(salt_raw);

    let mut result = argon2::hash_raw(
        password,
        salt.as_bytes(),
        &argon2::Config {
            hash_length: CRYPTO_DIGEST_SIZE as u32,
            variant: argon2::Variant::Argon2id,
            ..argon2::Config::default()
        },
    )?;

    let mut outbuf = [0; CRYPTO_DIGEST_SIZE];
    outbuf.copy_from_slice(&result);
    result.zeroize();

    Ok(Secret::new(outbuf))
}

fn derive_subkey(key: &RawKey, ctx: &[u8]) -> Result<RawKey> {
    assert!(ctx.len() < 16);

    let mut outbuf = [0; CRYPTO_DIGEST_SIZE];
    outbuf.copy_from_slice(
        Blake2::new()
            .hash_length(CRYPTO_DIGEST_SIZE)
            .key(ctx)
            .hash(key.expose_secret())
            .as_bytes(),
    );

    Ok(Secret::new(outbuf))
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_object_encryption() {
        use super::{CryptoProvider, ObjectOperations};
        use crate::object::WriteObject;
        use secrecy::Secret;

        let key = Secret::new(*b"abcdef1234567890abcdef1234567890");
        let cleartext = b"the quick brown fox jumps over the lazy crab";
        let len = cleartext.len();

        let crypto = ObjectOperations::new(key);
        let mut obj = WriteObject::default();
        obj.reserve_tag();

        let slice: &mut [u8] = obj.as_mut();
        slice[..len].copy_from_slice(cleartext);

        crypto.encrypt_object(&mut obj);

        let mut decrypted = WriteObject::default();
        crypto.decrypt_object_into(decrypted.as_inner_mut(), obj.as_inner(), obj.id());

        // do it again, because reusing target buffers is fair game
        crypto.decrypt_object_into(decrypted.as_inner_mut(), obj.as_inner(), obj.id());

        assert_eq!(&decrypted.as_inner()[..len], &cleartext[..]);
    }

    #[test]
    fn test_chunk_encryption() {
        use super::{CryptoProvider, ObjectOperations};
        use crate::{chunks::RawChunkPointer, object::WriteObject};
        use secrecy::Secret;
        use std::io::Write;

        let key = Secret::new(*b"abcdef1234567890abcdef1234567890");
        let hash = b"1234567890abcdef1234567890abcdef";
        let cleartext = b"the quick brown fox jumps ";
        let size = cleartext.len();
        let crypto = ObjectOperations::new(key);
        let mut obj = WriteObject::default();

        let mut encrypted = cleartext.clone();
        let tag = crypto.encrypt_chunk(obj.id(), hash, &mut encrypted);
        let cp = RawChunkPointer {
            offs: 0,
            size: size as u32,
            hash: *hash,
            tag,
            ..RawChunkPointer::default()
        };
        obj.write(&encrypted).unwrap();

        let mut decrypted = vec![0; size + tag.len()];
        crypto.decrypt_chunk(&mut decrypted, obj.as_ref(), obj.id(), &cp);

        assert_eq!(&decrypted[..size], cleartext.as_ref());
    }
}
