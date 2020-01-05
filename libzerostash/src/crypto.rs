use crate::chunks::ChunkPointer;
use crate::objects::{Object, ObjectId, WriteObject};

use std::convert::TryInto;

use blake2::{Blake2s, Digest};
use failure::Fail;
use getrandom::getrandom;
use libc::c_char;
use libsodium_sys::{
    crypto_kdf_KEYBYTES, crypto_kdf_derive_from_key, crypto_pwhash, crypto_pwhash_PASSWD_MIN,
    crypto_pwhash_SALTBYTES, crypto_pwhash_alg_default, crypto_pwhash_memlimit_interactive,
    crypto_pwhash_opslimit_interactive,
};
use ring::aead;
use secrecy::{ExposeSecret, Secret};

pub const CRYPTO_DIGEST_SIZE: usize = 32;
pub type DigestFn = Blake2s;
pub type CryptoDigest = [u8; CRYPTO_DIGEST_SIZE];
pub type Tag = [u8; 16];
type Nonce = [u8; 12];
type Key = Secret<[u8; 32]>;

pub trait Random {
    fn fill(&self, buf: &mut [u8]);
}

pub trait CryptoProvider: Random + Clone + Send {
    fn encrypt_chunk(&self, object_id: &WriteObject, hash: &CryptoDigest, data: &mut [u8]) -> Tag;
    fn encrypt_object(&self, object: &mut WriteObject);

    fn decrypt_chunk<T: AsRef<[u8]>>(
        &self,
        target: &mut [u8],
        o: &Object<T>,
        chunk: &ChunkPointer,
    ) -> usize;

    fn decrypt_object_into<I: AsRef<[u8]>, O: AsMut<[u8]>>(
        &self,
        output: &mut Object<O>,
        obj: &Object<I>,
    );
}

#[derive(Debug, Fail)]
#[fail(display = "Key error")]
pub struct KeyError;

pub struct StashKey {
    master_key: Key,
}

impl StashKey {
    pub fn open_stash(
        username: impl AsRef<str>,
        password: impl AsRef<str>,
    ) -> Result<StashKey, KeyError> {
        let saltsize = crypto_pwhash_SALTBYTES.try_into().unwrap();

        let mut hasher = DigestFn::new();
        hasher.input(username.as_ref());
        let salt = hasher.result();

        derive_argon2(&salt[..saltsize], password.as_ref().as_bytes())
            .map(|k| StashKey { master_key: k })
    }

    pub fn root_object_id(&self) -> Result<ObjectId, KeyError> {
        derive_subkey(&self.master_key, 0, b"_0s_root")
            .map(|k| ObjectId::from_bytes(k.expose_secret()))
    }

    pub fn get_meta_crypto(&self) -> Result<impl CryptoProvider, KeyError> {
        derive_subkey(&self.master_key, 0, b"_0s_meta").map(ObjectOperations::new)
    }

    pub fn get_object_crypto(&self) -> Result<impl CryptoProvider, KeyError> {
        derive_subkey(&self.master_key, 0, b"_0s_obj_").map(ObjectOperations::new)
    }
}

#[derive(Clone)]
pub struct ObjectOperations {
    key: Key,
}

impl ObjectOperations {
    pub fn new(key: Key) -> ObjectOperations {
        ObjectOperations { key }
    }
}

impl Random for ObjectOperations {
    fn fill(&self, buf: &mut [u8]) {
        getrandom(buf).unwrap()
    }
}

impl CryptoProvider for ObjectOperations {
    fn encrypt_chunk(&self, object: &WriteObject, hash: &CryptoDigest, data: &mut [u8]) -> Tag {
        let aead = get_aead(derive_chunk_key(&self.key, hash));
        let tag = aead
            .seal_in_place_separate_tag(
                get_chunk_nonce(&object.id, data.len() as u32),
                aead::Aad::empty(),
                data,
            )
            .unwrap();

        let mut t = Tag::default();
        t.copy_from_slice(tag.as_ref());
        t
    }

    fn encrypt_object(&self, object: &mut WriteObject) {
        let aead = get_aead(self.key.clone());

        let tag = aead
            .seal_in_place_separate_tag(
                get_object_nonce(&object.id),
                aead::Aad::empty(),
                object.as_mut(),
            )
            .unwrap();

        object.write_tag(tag.as_ref());
    }

    fn decrypt_chunk<T: AsRef<[u8]>>(
        &self,
        target: &mut [u8],
        o: &Object<T>,
        chunk: &ChunkPointer,
    ) -> usize {
        let size = chunk.size as usize;
        let cyphertext_size = size + chunk.tag.len();

        assert!(target.len() >= cyphertext_size);

        let start = chunk.offs as usize;
        let end = start + size;

        target[..size].copy_from_slice(&o.buffer.as_ref()[start..end]);
        target[size..cyphertext_size].copy_from_slice(&chunk.tag);

        let aead = get_aead(derive_chunk_key(&self.key, &chunk.hash));
        aead.open_in_place(
            get_chunk_nonce(&o.id, chunk.size),
            aead::Aad::empty(),
            &mut target[..cyphertext_size],
        )
        .unwrap();

        size
    }

    fn decrypt_object_into<I: AsRef<[u8]>, O: AsMut<[u8]>>(
        &self,
        output: &mut Object<O>,
        obj: &Object<I>,
    ) {
        let buf: &mut [u8] = output.buffer.as_mut();
        buf.copy_from_slice(&obj.buffer.as_ref());

        let aead = get_aead(self.key.clone());
        aead.open_in_place(get_object_nonce(&obj.id), aead::Aad::empty(), buf)
            .unwrap();

        output.reserve_tag();
    }
}

#[inline]
fn get_aead(key: Key) -> aead::LessSafeKey {
    let key =
        aead::UnboundKey::new(&aead::CHACHA20_POLY1305, key.expose_secret()).expect("bad key");
    aead::LessSafeKey::new(key)
}

#[inline]
fn derive_chunk_key(key_src: &Key, hash: &CryptoDigest) -> Key {
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

fn derive_argon2(salt: &[u8], password: &[u8]) -> Result<Key, KeyError> {
    let mut outbuf = [0; crypto_kdf_KEYBYTES as usize];

    assert!(salt.len() == crypto_pwhash_SALTBYTES as usize);
    assert!(password.len() >= crypto_pwhash_PASSWD_MIN as usize);

    unsafe {
        if crypto_pwhash(
            outbuf.as_mut_ptr(),
            outbuf.len().try_into().unwrap(),
            password.as_ptr() as *const c_char,
            password.len().try_into().unwrap(),
            salt.as_ptr(),
            crypto_pwhash_opslimit_interactive().try_into().unwrap(),
            crypto_pwhash_memlimit_interactive(),
            crypto_pwhash_alg_default(),
        ) != 0
        {
            return Err(KeyError);
        }
    }

    Ok(Secret::new(outbuf))
}

fn derive_subkey(key: &Key, subkey_id: u64, ctx: &[u8]) -> Result<Key, KeyError> {
    let mut outbuf = [0; CRYPTO_DIGEST_SIZE];

    assert!(ctx.len() == 8);

    unsafe {
        if crypto_kdf_derive_from_key(
            outbuf.as_mut_ptr(),
            outbuf.len().try_into().unwrap(),
            subkey_id,
            ctx.as_ptr() as *const c_char,
            key.expose_secret().as_ptr(),
        ) != 0
        {
            return Err(KeyError);
        }
    }

    Ok(Secret::new(outbuf))
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_object_encryption() {
        use super::{CryptoProvider, ObjectOperations};
        use crate::objects::WriteObject;
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
        crypto.decrypt_object_into(&mut decrypted, &obj);

        // do it again, because reusing target buffers is fair game
        crypto.decrypt_object_into(&mut decrypted, &obj);

        assert_eq!(&decrypted.buffer.as_ref()[..len], cleartext.as_ref());
    }

    #[test]
    fn test_chunk_encryption() {
        use super::{ChunkPointer, CryptoProvider, ObjectOperations};
        use crate::objects::WriteObject;
        use secrecy::Secret;
        use std::io::Write;

        let key = Secret::new(*b"abcdef1234567890abcdef1234567890");
        let hash = b"1234567890abcdef1234567890abcdef";
        let cleartext = b"the quick brown fox jumps ";
        let size = cleartext.len();
        let crypto = ObjectOperations::new(key);
        let mut obj = WriteObject::default();

        let mut encrypted = cleartext.clone();
        let tag = crypto.encrypt_chunk(&obj, hash, &mut encrypted);
        let cp = ChunkPointer {
            offs: 0,
            size: size as u32,
            hash: *hash,
            tag,
            ..ChunkPointer::default()
        };
        obj.write(&encrypted).unwrap();

        let mut decrypted = vec![0; size + tag.len()];
        crypto.decrypt_chunk(&mut decrypted, &obj, &cp);

        assert_eq!(&decrypted[..size], cleartext.as_ref());
    }
}
