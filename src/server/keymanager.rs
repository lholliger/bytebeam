use std::collections::HashMap;
use ssh_key::{PublicKey, SshSig};
use tracing::{debug, error, warn};

// this handles all signing operations
#[derive(Debug, Clone)]
pub struct KeyManager {
    keyserver: Option<String>, // for example. github does https://github.com/username.keys
    users: HashMap<String, Vec<PublicKey>> // allowed users, and all of their keys. If no keyserver, this comes from a config
}

impl KeyManager {
    pub async fn new_checking_keyserver(keyserver: Option<String>, users: Vec<String>) -> Self {
        let mut km = KeyManager {
            keyserver,
            users: HashMap::new(),
        };

        // we need to see if "users" is a list of SSH keys or simply just a list of usernames which we ask the keyserver for
        // users can exist as SSH keys, using the keyserver by no means says you cannot also have hardcoded user keys
        for user in users {
            match PublicKey::from_openssh(&user) {
                Ok(key) => {
                    debug!("User provided has SSH key {}", key.fingerprint(Default::default()));
                    km.users.insert(user.clone(), vec![key]);
                },
                Err(_) => {
                    // ssh_key::authorized_keys
                    // if we can't parse the key, it's probably a username and we need to ask the keyserver for their keys
                    debug!("Getting {}'s keys from keyserver", user);
                    let response = km.get_keys_from_keyserver(&user).await;
                    if let Some(key_response) = response {
                        km.users.insert(user.clone(), key_response);
                    } else {
                        error!("Failed to get keyserver keys!");
                    }
                },
            }
        }

        km
    }

    async fn get_keys_from_keyserver(&self, name: &String) -> Option<Vec<PublicKey>> {
        if self.keyserver.is_none() {
            return None;
        }
        let ks = self.keyserver.as_ref().unwrap();
        let url = ks.replace("{}", name);
        debug!("Checking key server at {} for user {}", url, name);
        return match reqwest::get(url).await {
            Ok(response) => {
                if response.status().is_success() {
                    let keys_str = match response.text().await {
                        Ok(s) => s,
                        Err(e) => {
                            error!("Failed to read response text from keyserver: {:?}", e);
                            return None;
                        },
                    };
                    let keys = ssh_key::authorized_keys::AuthorizedKeys::new(&keys_str);
                    let mut o_keys = vec![];
                    for key in keys {
                        match key {
                            Ok(k) => o_keys.push(k.public_key().clone()),
                            Err(e) => warn!("Could not parse SSH key from keyserver: {:?}", e)
                        }
                    }
                    Some(o_keys)
                } else {
                    None
                }
            },
            Err(e) => {
                error!("Could not get data from keyserver: {:?}", e);
                None
            }
        };
    }

    pub fn verify(&self, name: &String, challenge: &String, response: &String) -> bool {
        let user_keys = match self.users.get(name) {
            Some(keys) => keys,
            None => return false,
        };

        let signature = match response.parse::<SshSig>() {
            Ok(s) => s,
            Err(e) => {
                error!("Failed to parse SSH challenge: {:?}", e);
                return false;
            },
        };

        for key in user_keys {
            match key.verify("bytebeam", challenge.as_bytes(), &signature) {
                Ok(_) => return true, // we only need it to succeed once!
                Err(e) => debug!("Failed to verify SSH key: {:?}", e)
            }
        }

        return false;
    }
}