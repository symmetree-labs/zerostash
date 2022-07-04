use super::{KeyToSource, Result};
use infinitree::keys::{KeySource, UsernamePassword};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};

/// Contents of a key file
#[derive(clap::Args, Default, Clone, Debug, Deserialize, Serialize)]
pub struct SymmetricKey {
    #[serde(serialize_with = "ser_secret_string")]
    pub user: Option<SecretString>,

    #[serde(serialize_with = "ser_secret_string")]
    #[clap(skip)]
    pub password: Option<SecretString>,
}

impl KeyToSource for SymmetricKey {
    fn to_keysource(self, _stash_name: &str) -> Result<KeySource> {
        let (user, pw) = self.ensure_credentials()?;
        Ok(UsernamePassword::with_credentials(user, pw)?)
    }
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
