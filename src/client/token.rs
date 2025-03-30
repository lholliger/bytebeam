use std::{fs, path::{Path, PathBuf}};

use ssh_key::{PrivateKey, SshSig};
use tracing::{debug, error, trace, warn};

use crate::utils::metadata::FileMetadata;

pub async fn get_upload_token(username: &String, file_len: usize, request_path: String) -> Option<FileMetadata> {
    let params = [("user", username.clone()), ("file-size", file_len.to_string())];

    let client = reqwest::Client::new();
    let res = client.post(request_path)
        .form(&params)
        .send().await;

    debug!("Request: {:?}", res);

    let parsed = parse_response(res).await;

    match parsed {
        Some(metadata) => {
            debug!("File metadata received: {:?}", metadata);
            Some(metadata)
        },
        None => {
            error!("Error parsing response");
            None
        }
    }
}


async fn parse_response(res: Result<reqwest::Response, reqwest::Error>) -> Option<FileMetadata> {
    match res {
        Ok(response) => {
            if !response.status().is_success() {
                error!(
                    "Non-success response from Beam server: {:?}", response.text().await
                );
                return None;
            }
            let wanted_version = format!("ByteBeam/{}", env!("CARGO_PKG_VERSION"));
            // warn if the versions are different
            match response.headers().get("server") {
                Some(version) => match version.to_str() {
                    Ok(version_str) => if version_str != wanted_version {
                        warn!("ByteBeam Server version does not match the expected version. It may be outdated and there may be instability! Got {}, wanted {}", version_str, wanted_version);
                    }
                    Err(_) => warn!("ByteBeam Server did not return a proper version string. It may be outdated and there may be instability!")
                }
                None => {
                    warn!("ByteBeam Server did not return a version. It may be outdated and there may be instability!");
                }
            }
            match response.json::<FileMetadata>().await {
                Ok(metadata) => Some(metadata),
                Err(e) => {
                    error!("Failed to parse file metadata: {:?}.", e);
                    return None;
                }
            }
        },
        Err(e) => {
            error!("Failed to connect to Beam server: {:?}", e);
            return None;
        }
    }
}

pub async fn get_upgrade(current_path: &String, challenge: &Vec<String>) -> Option<FileMetadata> {
    let cstr = match serde_json::to_string(&challenge) {
        Ok(cstr) => cstr,
        Err(_) => {
            error!("Could not convert challenge to JSON");
            return None
        }
    };
    let params = [("challenge", cstr)];

    let client = reqwest::Client::new();
    let res = client.post(current_path)
        .form(&params)
        .send().await;

        debug!("Request: {:?}", res);

        let parsed = parse_response(res).await;
    
        match parsed {
            Some(metadata) => {
                debug!("File metadata received: {:?}", metadata);
                Some(metadata)
            },
            None => {
                error!("Error parsing response");
                None
            }
        }
}

pub fn sign_challenge(challenge: &String, keys: &Vec<PrivateKey>) -> Vec<SshSig> {
    let mut output = vec![];
    for key in keys {
        match key.sign("bytebeam", ssh_key::HashAlg::Sha512, challenge.as_bytes()) {
            Ok(signature) => {
                debug!("Signed {} with key: {}", challenge, key.fingerprint(ssh_key::HashAlg::Sha512));
                output.push(signature);
            },
            Err(e) => error!("Failed to sign with key: {:?}", e),
        }
    }
    output
}

pub fn get_privkey(data: &String) -> Option<PrivateKey> {
    match ssh_key::PrivateKey::from_openssh(data) {
        Ok(key) => Some(key),
        Err(e) => {
            error!("Failed to parse private key: {:?}", e);
            None
        }
    }
}

pub fn get_key_or_keys_from_path(path: &Path) -> Vec<PrivateKey> {
    let mut output = vec![];
    // test if a folder
    if path.is_dir() { // we need to scan each file now
        // get files in dir
        let entries = match fs::read_dir(path) {
            Ok(entries) => entries,
            Err(e) => {
                error!("Failed to read key directory: {:?}", e);
                return vec![];
            }  
        };

        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(e) => {
                    error!("Failed to read entry: {:?}", e);
                    continue
                }  
            };

            let entry_details = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(e) => {
                    error!("Failed to read entry details: {:?}", e);
                    continue
                }  
            };

            if entry_details.is_file() {
                let file_path = entry.path();
                let data = match fs::read_to_string(&file_path) {
                    Ok(data) => data,
                    Err(e) => {
                        error!("Failed to read file: {:?}", e);
                        continue
                    }  
                };
                match get_privkey(&data) {
                    Some(key) => output.push(key),
                    None => error!("Failed to parse private key from file: {:?}", file_path),
                }
            }
        }
    } else { // we need to check if it is a file
        let data = fs::read_to_string(path).expect("Failed to read file");
        match get_privkey(&data) {
            Some(key) => output.push(key),
            None => error!("Failed to parse private key from file: {:?}", path),
        }
    }

    output
}

pub async fn do_run_upgrade_on_metadata(metadata: FileMetadata, username: &String, key: &String, server: &String) -> FileMetadata {
    if *username != "default".to_string() { // this is worth authentication now
        // we need to expand the key
        let expanded = shellexpand::tilde(&key).into_owned();
        let config_path = PathBuf::new().join(&expanded);
        let keys = get_key_or_keys_from_path(&config_path);
        let challenges = match metadata.get_challenge_details() {
            Some(challenge) => {
                if *username != challenge.1.clone() {
                    warn!("Username mismatch for challenge. Expected {}, got {}.", username, challenge.1)
                }
                sign_challenge(challenge.2, &keys)
            },
            None => {
                error!("Failed to get challenge details from server. Is the server up to date?");
                return metadata
            }
        };
        // now we can try to update things
        if challenges.len() == 0 {
            warn!("Could not sign the challenge, running with no authentication!");
            return metadata
        } else {
            let mut testing_val = vec![];
            for chal in challenges {
                match chal.to_pem(ssh_key::LineEnding::default()) {
                    Ok(pem) => testing_val.push(pem),
                    Err(e) => error!("Failed to parse PEM: {}", e),
                }
            }


            match get_upgrade(&format!("{server}/{}", metadata.get_upload_info().0), &testing_val).await {
                Some(meta) => {
                    if !meta.authenticated() {
                        warn!("Server returned metadata but it was not authenticated! Proceeding with new data!");
                    } else {
                        debug!("Authenticated! Switching metadata");
                    }
                    return meta
                },
                None => {
                    warn!("Could not properly authenticate, proceeding normally!");
                    return metadata
                }
            }
        }
    }
    trace!("Using default user. No authentication will happen");
    return metadata
}