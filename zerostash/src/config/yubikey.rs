use super::{symmetric_key::SymmetricKey, KeyToSource};
use anyhow::Result;
use infinitree::keys::yubikey::{
    yubico_manager::{config::*, *},
    YubikeyCR,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YubikeyCRKey {
    #[serde(flatten)]
    pub credentials: SymmetricKey,

    #[serde(flatten)]
    pub config: YubikeyCRConfig,
}

impl KeyToSource for YubikeyCRKey {
    type Target = YubikeyCR;

    fn to_keysource(self, stash: &str) -> Result<Self::Target> {
        let mut yk = Yubico::new();
        let device = yk.find_yubikey()?;

        let mut ykconfig = Config::default()
            .set_product_id(device.product_id)
            .set_vendor_id(device.vendor_id);

        if let Some(slot) = self.config.slot {
            ykconfig = ykconfig.set_slot(match slot {
                YubikeyCRSlot::Slot1 => Slot::Slot1,
                YubikeyCRSlot::Slot2 => Slot::Slot2,
            });
        }
        if let Some(key) = self.config.key {
            ykconfig = ykconfig.set_command(match key {
                YubikeyCRHmac::Hmac1 => Command::ChallengeHmac1,
                YubikeyCRHmac::Hmac2 => Command::ChallengeHmac2,
            });
        }

        let (user, pw) = self.credentials.interactive_credentials(stash)?;
        Ok(YubikeyCR::with_credentials(user, pw, ykconfig)?)
    }
}

/// Contents of a key file
#[derive(clap::Args, Default, Clone, Debug, Deserialize, Serialize)]
pub struct YubikeyCRConfig {
    #[serde(default)]
    #[clap(value_enum)]
    pub slot: Option<YubikeyCRSlot>,

    #[serde(default)]
    #[clap(value_enum)]
    pub key: Option<YubikeyCRHmac>,
}

#[derive(clap::ValueEnum, Clone, Debug, Deserialize, Serialize)]
pub enum YubikeyCRSlot {
    #[serde(rename = "slot1")]
    Slot1,
    #[serde(rename = "slot2")]
    Slot2,
}

#[derive(clap::ValueEnum, Clone, Debug, Deserialize, Serialize)]
pub enum YubikeyCRHmac {
    #[serde(rename = "hmac1")]
    Hmac1,
    #[serde(rename = "hmac2")]
    Hmac2,
}
