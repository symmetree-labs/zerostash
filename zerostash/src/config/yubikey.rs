use serde::{Deserialize, Serialize};

/// Contents of a key file
#[derive(Default, Clone, Debug, Deserialize, Serialize)]
pub struct YubikeyCRConfig {
    #[serde(default)]
    pub slot: Option<YubikeyCRSlot>,
    #[serde(default)]
    pub key: Option<YubikeyCRKey>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum YubikeyCRSlot {
    #[serde(rename = "slot1")]
    Slot1,
    #[serde(rename = "slot2")]
    Slot2,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum YubikeyCRKey {
    #[serde(rename = "hmac1")]
    Hmac1,
    #[serde(rename = "hmac2")]
    Hmac2,
}
