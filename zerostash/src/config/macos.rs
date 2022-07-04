use super::*;
use infinitree::keys::UsernamePassword;
use serde::{Deserialize, Serialize};

#[derive(clap::Args, Clone, Debug, Deserialize, Serialize)]
pub struct KeychainCredentials {
    pub user: String,
}

impl KeyToSource for KeychainCredentials {
    fn to_keysource(self, stash: &str) -> Result<KeySource> {
        let service_name = "dev.symmetree.zerostash";
        let account_name = format!("{}#:0s:#{}", stash, self.user);

        let pass = security_framework::passwords::get_generic_password(service_name, &account_name)
            .map(|pass| String::from_utf8_lossy(&pass).to_string())
            .unwrap_or_else(|_| {
                println!("Enter a new password to save in Keychain!");
                println!("Press enter to generate a strong random password.");
                let pw = {
                    let pw = rpassword::prompt_password("Password: ").expect("Invalid password");

                    if pw.is_empty() {
                        UsernamePassword::generate_password().expect("Random error")
                    } else {
                        pw
                    }
                };

                security_framework::passwords::set_generic_password(
                    service_name,
                    &account_name,
                    pw.as_bytes(),
                )
                .expect("Failed to add password to keychain!");

                pw
            });

        Ok(UsernamePassword::with_credentials(
            self.user.into(),
            pass.into(),
        )?)
    }
}
