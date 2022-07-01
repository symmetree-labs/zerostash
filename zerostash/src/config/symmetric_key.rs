use super::Result;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};

/// Contents of a key file
#[derive(Default, Clone, Debug, Deserialize, Serialize)]
pub struct SymmetricKey {
    #[serde(serialize_with = "ser_secret_string")]
    user: Option<SecretString>,
    #[serde(serialize_with = "ser_secret_string")]
    password: Option<SecretString>,
}

impl SymmetricKey {
    /// Ask for credentials on the standard input using [rpassword]
    pub fn ensure_credentials(self) -> Result<(SecretString, SecretString)> {
        let user = match self.user {
            Some(u) => u,
            None => rprompt::prompt_reply_stderr("Username: ")?.into(),
        };

        let pass = match self.password {
            Some(p) => p,
            None => rprompt::prompt_reply_stderr("Password: ")?.into(),
        };

        Ok((user, pass))
    }
}

/// panics currently
fn ser_secret_string<S>(val: &Option<SecretString>, ser: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    ser.serialize_str(val.as_ref().unwrap().expose_secret())
}
