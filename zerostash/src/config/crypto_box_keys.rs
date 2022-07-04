use super::*;
use bech32::{FromBase32, ToBase32};
use infinitree::keys::{crypto_box::StorageOnly, RawKey};
use secrecy::ExposeSecret;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(clap::Args, Debug, Clone, Serialize, Deserialize)]
pub struct SplitKeyStorage {
    #[serde(flatten)]
    #[clap(flatten)]
    pub credentials: SymmetricKey,

    #[serde(flatten)]
    #[clap(flatten)]
    pub keys: SplitKeys,
}

impl KeyToSource for SplitKeyStorage {
    fn to_keysource(self, _stash_name: &str) -> Result<KeySource> {
        let (user, pw) = self.credentials.ensure_credentials()?;

        Ok(match self.keys.read {
            Some(sk) => StorageOnly::encrypt_and_decrypt(user, pw, self.keys.write, sk),
            None => StorageOnly::encrypt_only(user, pw, self.keys.write),
        }?)
    }
}

#[derive(clap::Args, Clone, Deserialize, Serialize)]
pub struct SplitKeys {
    #[serde(
        serialize_with = "ser_public_key",
        deserialize_with = "deser_public_key"
    )]
    pub write: RawKey,
    #[serde(
        serialize_with = "ser_option_secret_key",
        deserialize_with = "deser_option_secret_key",
        default
    )]
    pub read: Option<RawKey>,
}

fn bech32_pk(k: &RawKey) -> String {
    bech32::encode(
        "p0s-",
        k.expose_secret().to_base32(),
        bech32::Variant::Bech32m,
    )
    .unwrap()
}

fn bech32_sk(k: &RawKey) -> String {
    bech32::encode(
        "s0s-",
        k.expose_secret().to_base32(),
        bech32::Variant::Bech32m,
    )
    .unwrap()
}

fn decode_bech32(check_hrp: &str, ser: &str) -> Result<RawKey> {
    let (hrp, data, _) = bech32::decode(ser)?;
    let bytes = Vec::from_base32(&data)?;
    if bytes.len() != 32 {
        anyhow::bail!("invalid key length");
    }

    if check_hrp != hrp {
        anyhow::bail!("invalid key type");
    }

    let mut target = [0; 32];
    target.copy_from_slice(&bytes);
    Ok(target.into())
}

impl std::fmt::Debug for SplitKeys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SplitKeys")
            .field("write", &bech32_pk(&self.write))
            .field("read", &self.read.is_some())
            .finish()
    }
}

fn ser_public_key<S>(val: &RawKey, ser: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    ser.serialize_str(&bech32_pk(val))
}

fn ser_option_secret_key<S>(val: &Option<RawKey>, ser: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    match val {
        Some(ref k) => ser.serialize_some(&bech32_sk(k)),
        None => ser.serialize_none(),
    }
}

fn deser_public_key<'de, D>(deser: D) -> Result<RawKey, D::Error>
where
    D: Deserializer<'de>,
{
    let s: &str = serde::de::Deserialize::deserialize(deser)?;
    decode_bech32("p0s-", s).map_err(serde::de::Error::custom)
}

fn deser_option_secret_key<'de, D>(deser: D) -> Result<Option<RawKey>, D::Error>
where
    D: Deserializer<'de>,
{
    let s: Option<&str> = serde::de::Deserialize::deserialize(deser)?;
    s.map(|k| decode_bech32("s0s-", k))
        .transpose()
        .map_err(serde::de::Error::custom)
}
