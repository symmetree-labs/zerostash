use super::{KeyToSource, Result};
use infinitree::keys::UsernamePassword;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};

const SERVICE_NAME: &str = "dev.symmetree.zerostash";

/// Username + Password information
#[derive(clap::Args, Default, Clone, Debug, Deserialize, Serialize)]
pub struct SymmetricKey {
    /// Username
    #[serde(serialize_with = "ser_secret_string")]
    #[clap(short, long)]
    pub user: Option<SecretString>,

    /// Password
    #[serde(
        serialize_with = "ser_secret_string",
        skip_serializing_if = "Option::is_none"
    )]
    #[clap(skip)]
    pub password: Option<SecretString>,

    /// Use macOS Keychain for storing the password
    #[serde(default, skip_serializing_if = "is_false")]
    #[clap(short = 'e', long = "keychain")]
    pub keychain: bool,
}

fn is_false(v: &bool) -> bool {
    !v
}

impl KeyToSource for SymmetricKey {
    type Target = UsernamePassword;

    fn to_keysource(self, stash: &str) -> Result<Self::Target> {
        let (user, pw) = self.interactive_credentials(stash)?;
        Ok(UsernamePassword::with_credentials(user, pw)?)
    }
}

impl SymmetricKey {
    pub fn is_empty(&self) -> bool {
        self.user.is_none() && self.password.is_none()
    }

    /// Fill in missing fields using random generator
    pub fn fill_random(mut self, stash: &str) -> Result<Self> {
        // technically this shouldn't run, just being pedantic
        // clap rules should enforce this always being provided on cli
        if self.user.is_none() {
            self.user = Some(UsernamePassword::generate_password()?.into());
        }

        if self.password.is_none() {
            let pw: SecretString = UsernamePassword::generate_password()?.into();
            if cfg!(target_os = "macos") && self.keychain {
                set_keychain_pw(
                    stash,
                    self.user.as_ref().unwrap().expose_secret(),
                    pw.expose_secret(),
                )?;
            } else {
                self.password = Some(pw);
            }
        }

        Ok(self)
    }

    /// Ask for credentials on the standard input using [rpassword]
    pub fn interactive_credentials(self, stash: &str) -> Result<(SecretString, SecretString)> {
        let user = match self.user {
            Some(ref u) => u.clone(),
            None => rprompt::prompt_reply_stderr("Username: ")?.into(),
        };

        let pass = if cfg!(target_os = "macos") && self.keychain {
            ask_keychain_pass(stash, &user)?
        } else {
            match self.password {
                Some(p) => p,
                None => rprompt::prompt_reply_stderr("Password: ")?.into(),
            }
        };

        Ok((user, pass))
    }
}

#[cfg(target_os = "macos")]
fn ask_keychain_pass(stash: &str, user: &SecretString) -> Result<SecretString> {
    let pw = get_keychain_pw(stash, user.expose_secret());

    match pw {
        ok @ Ok(_) => ok,
        Err(_) => {
            println!("Enter a new password to save in Keychain!");
            println!("Press enter to generate a strong random password.");
            let pw = {
                let pw = rpassword::prompt_password("Password: ")?;

                if pw.is_empty() {
                    UsernamePassword::generate_password()?
                } else {
                    pw
                }
            };

            set_keychain_pw(stash, user.expose_secret(), &pw)?;

            Ok(pw.into())
        }
    }
}

#[cfg(target_os = "macos")]
fn get_keychain_pw(stash: &str, user: &str) -> Result<SecretString> {
    use security_framework::passwords::get_generic_password;
    let account_name = format!("{}#:0s:#{}", stash, user);

    Ok(get_generic_password(SERVICE_NAME, &account_name)
        .map(|pass| SecretString::new(String::from_utf8_lossy(&pass).to_string()))?)
}

#[cfg(target_os = "macos")]
fn set_keychain_pw(stash: &str, user: &str, pw: &str) -> Result<()> {
    use security_framework::passwords::set_generic_password;
    let account_name = format!("{}#:0s:#{}", stash, user);

    Ok(set_generic_password(
        SERVICE_NAME,
        &account_name,
        pw.as_bytes(),
    )?)
}

fn ser_secret_string<S>(val: &Option<SecretString>, ser: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    ser.serialize_str(val.as_ref().unwrap().expose_secret())
}
