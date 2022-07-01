use super::Result;
use super::{crypto_box_keys::*, symmetric_key::*, yubikey::*};
use infinitree::keys::{yubikey::YubikeyCR, KeySource, UsernamePassword};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Credentials for a stash
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[serde(tag = "source")]
pub enum Key {
    /// Plain text username/password pair
    #[serde(rename = "plaintext")]
    #[allow(missing_docs)]
    Plaintext(SymmetricKey),

    /// Get credentials through other interactive/command line methods
    #[serde(rename = "ask")]
    Interactive,

    /// Plain text username/password pair
    #[serde(rename = "file")]
    #[allow(missing_docs)]
    KeyFile { path: PathBuf },

    /// User/Password pair + 2FA
    #[serde(rename = "yubikey")]
    #[allow(missing_docs)]
    YubiKey {
        #[serde(flatten)]
        credentials: SymmetricKey,
        #[serde(flatten)]
        config: YubikeyCRConfig,
    },

    /// Use a different key for reading archives and appending
    #[serde(rename = "split_key")]
    #[allow(missing_docs)]
    SplitKeyStorage {
        #[serde(flatten)]
        credentials: SymmetricKey,
        #[serde(flatten)]
        keys: SplitKeys,
    },

    #[cfg(target_os = "macos")]
    #[serde(rename = "macos_keychain")]
    MacOsKeychain { user: String },
}

impl Key {
    // On non-macos the parameter will generate a warning.
    #[allow(unused)]
    pub(super) fn get_credentials(self, stash: &str) -> Result<KeySource> {
        match self {
            Self::Interactive => {
                let (user, pw) = SymmetricKey::default().ensure_credentials()?;
                Ok(UsernamePassword::with_credentials(user, pw)?)
            }
            Self::Plaintext(k) => {
                let (user, pw) = k.ensure_credentials()?;
                Ok(UsernamePassword::with_credentials(user, pw)?)
            }
            Self::KeyFile { path } => {
                let contents = std::fs::read_to_string(path)?;
                let keys: Key = toml::from_str(&contents)?;

                // this is technically recursion, it may be an ouroboros
                keys.get_credentials(stash)
            }
            Self::YubiKey {
                credentials,
                config,
            } => {
                use infinitree::keys::yubikey::yubico_manager::{config::*, *};
                let mut yk = Yubico::new();
                let device = yk.find_yubikey()?;

                let mut ykconfig = Config::default()
                    .set_product_id(device.product_id)
                    .set_vendor_id(device.vendor_id);

                if let Some(slot) = config.slot {
                    ykconfig = ykconfig.set_slot(match slot {
                        YubikeyCRSlot::Slot1 => Slot::Slot1,
                        YubikeyCRSlot::Slot2 => Slot::Slot2,
                    });
                }
                if let Some(key) = config.key {
                    ykconfig = ykconfig.set_command(match key {
                        YubikeyCRKey::Hmac1 => Command::ChallengeHmac1,
                        YubikeyCRKey::Hmac2 => Command::ChallengeHmac2,
                    });
                }

                let (user, pw) = credentials.ensure_credentials()?;
                Ok(YubikeyCR::with_credentials(user, pw, ykconfig)?)
            }
            Self::SplitKeyStorage { credentials, keys } => {
                let (user, pw) = credentials.ensure_credentials()?;

                Ok(match keys.read {
                    Some(sk) => infinitree::keys::crypto_box::StorageOnly::encrypt_and_decrypt(
                        user, pw, keys.write, sk,
                    ),
                    None => infinitree::keys::crypto_box::StorageOnly::encrypt_only(
                        user, pw, keys.write,
                    ),
                }?)
            }

            #[cfg(target_os = "macos")]
            Self::MacOsKeychain { user } => {
                let service_name = "dev.symmetree.zerostash";
                let account_name = format!("{}#:0s:#{}", stash, user);

                let pass = security_framework::passwords::get_generic_password(
                    service_name,
                    &account_name,
                )
                .map(|pass| String::from_utf8_lossy(&pass).to_string())
                .unwrap_or_else(|_| {
                    println!("Enter a new password to save in Keychain!");
                    println!("Press enter to generate a strong random password.");
                    let pw = rpassword::prompt_password("Password: ").expect("Invalid password");

                    security_framework::passwords::set_generic_password(
                        service_name,
                        &account_name,
                        pw.as_bytes(),
                    )
                    .expect("Failed to add password to keychain!");

                    pw
                });

                Ok(UsernamePassword::with_credentials(
                    user.into(),
                    pass.into(),
                )?)
            }
        }
    }
}
