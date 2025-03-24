use std::{collections::HashMap, sync::Arc, thread};
use reqwest::StatusCode;
use tokio::sync::{mpsc::{channel, Receiver, Sender}, Mutex};
use tracing::{debug, trace};

use crate::utils::{compression::Compression, metadata::FileMetadata};

use super::{keymanager::KeyManager, serveropts::ServerOptions};

#[derive(Debug, Clone)]
pub struct AppState {
    files: Arc<Mutex<HashMap<String, FileMetadata>>>,
    downloads: Arc<Mutex<HashMap<String, Receiver<Vec<u8>>>>>,
    uploads: Arc<Mutex<HashMap<String, Sender<Vec<u8>>>>>,
    reg_options: ServerOptions, // for all users w/o keysigning
    auth_options: ServerOptions, // for verified users
    keys: KeyManager
}

impl AppState {
    pub async fn new(reg_options: ServerOptions, auth_options: ServerOptions, keyserver: Option<String>, users: Vec<String>) -> Self {
        let state = AppState {
            files: Arc::new(Mutex::new(HashMap::new())),
            downloads: Arc::new(Mutex::new(HashMap::new())),
            uploads: Arc::new(Mutex::new(HashMap::new())),
            keys: KeyManager::new_checking_keyserver(keyserver, users).await,
            reg_options,
            auth_options
        };

        let cull_state = state.clone();
        thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                trace!("Starting cull loop");
                loop {
                    tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
                    let culls = cull_state.cull().await;
                    if culls > 0 {
                        debug!("Culled {} uploads (expired)", culls);
                    }
                }
            });
        });

        state
    }

    pub async fn generate_file_upload(&self, file_name: &String, user: Option<&String>) -> Option<FileMetadata> {
        let mut uploads = self.uploads.lock().await;
        let mut downloads = self.downloads.lock().await;
        let mut meta = self.files.lock().await;
        let (tx, rx) = channel(self.reg_options.get_cache_size()); // TODO: this should be a whole pool instead of just per-request
    
        let mut upload = FileMetadata::new(&self.reg_options, user);

        upload.file_name = file_name.clone();//.split_off(40);
    
        uploads.insert(upload.get_token().clone(), tx);
        downloads.insert(upload.get_token().clone(), rx);

        meta.insert(upload.get_token().clone(), upload.clone());        
        Some(upload)
    }

    // this will upgrade the user's file upload if their authentication challenge succeeds
    pub async fn upgrade(&self, ticket: &String, challenge_responses: &Vec<String>) -> Option<FileMetadata> {
        let mut meta = self.files.lock().await;
        let file = meta.get(ticket);
        match file {
            Some(file) => {
                match file.get_challenge_details() {
                    Some((authenticated, user, challenge)) => {
                        for challenge_response in challenge_responses {
                            if authenticated {
                                // its already upgraded
                                return Some(file.clone());
                            }

                            if self.keys.verify(&user, &challenge, challenge_response) {
                                // now we need to move everything around and upgrade to authed
                                // ticket is still the old token
                                let mut file = file.clone();
                                file.upgrade(&self.auth_options);
                                // now we need to move everything around and upgrade to authed
                                let mut uploads = self.uploads.lock().await;
                                let mut downloads = self.downloads.lock().await;

                                let (tx, rx) = channel(self.auth_options.get_cache_size());
                                match uploads.remove(ticket) {
                                    Some(tik) => {
                                        // if it has been used, we cannot re-create it!
                                        if tik.capacity() != self.reg_options.get_cache_size() {
                                            uploads.insert(file.get_token().clone(), tik);
                                        } else {
                                            uploads.insert(file.get_token().clone(), tx);
                                            downloads.insert(ticket.to_string(), rx); // this will just cause a nice simple move and override the old one
                                        }
                                    },
                                    None => ()
                                };
                                match downloads.remove(ticket) {
                                    Some(tik) => {
                                        downloads.insert(file.get_token().clone(), tik);
                                    },
                                    None => ()
                                };
                                match meta.remove(ticket) {
                                    Some(_) => {
                                        meta.insert(file.get_token().clone(), file.clone());
                                    },
                                    None => ()
                                };

                                return Some(file);
                            } else {
                                return None;
                            }
                        }
                        return None;
                    },
                    None => None
                }
            },
            None => None,
        }
    }

    pub async fn get_file_metadata(&self, ticket: &String) -> Option<FileMetadata> {
        trace!("Attempting to get metadata for {}", ticket);
        let mut meta = self.files.lock().await;
        let file = meta.get_mut(ticket);
        match file {
            Some(file) => {
                trace!("Updating access time for {}", ticket);
                file.access();
                Some(file.clone())
            },
            None => None,
        }
    }

    // this gets a bit weird since it uses the FileMetadata as its own thing so it could get messy when the start_upload is triggered but the upload doesnt exist in self here
    pub async fn begin_upload(&self, ticket: &String, key: &String) -> Result<(Sender<Vec<u8>>, &ServerOptions), (StatusCode, String)> {
        match self.files.lock().await.get_mut(ticket) { // need mut just in case the upload is valid, so we can instantly lock it
            Some(meta) => {
                if meta.upload_locked() { // cannot allow another upload
                    Err((StatusCode::CONFLICT,"File is already locked for upload".to_string()))
                } else if !meta.check_key(key) {
                    return Err((StatusCode::FORBIDDEN, "File has a different key".to_string()))
                } else {
                    // okay, we've verified the upload so now we can lock it
                    match self.uploads.lock().await.get(ticket) {
                        Some(tx) => {
                            let opts = if meta.authenticated() {
                                &self.auth_options
                            } else {
                                &self.reg_options
                            };
                            meta.start_upload(key);
                            Ok((tx.clone(), opts)) // yay!
                        },
                        None => Err((StatusCode::GONE, "Upload does not exist, it is already in progress".to_string()))
                    }
                }
            },
            None => Err((StatusCode::NOT_FOUND, "Upload ticket does not exist".to_string()))
        }
    }

    pub async fn begin_download(&self, ticket: &String) -> Option<Receiver<Vec<u8>>> {
        match self.files.lock().await.get_mut(ticket) { // downloads are kinda weird since they need to be lockable and unlockable, however the lock must consume as this isnt a broadcast
            Some(meta) => {
                if meta.download_locked() { // cannot allow another download
                    None
                } else {
                    // okay, we've verified the upload so now we can lock it
                    match self.downloads.lock().await.remove(ticket) {
                        Some(rx) => {
                            meta.start_download();
                            Some(rx) // yay!
                        },
                        None => None
                    }
                }
            },
            None => None
        }
    }

    pub async fn return_download(&self, ticket: &String, stream: Receiver<Vec<u8>>) -> bool {
        match self.files.lock().await.get_mut(ticket) {
            Some(meta) => {
                if meta.download_pausable() {
                    self.downloads.lock().await.insert(ticket.clone(), stream);
                    meta.pause_download();
                    true
                } else {
                    false
                }
            },
            None => false
        }
    }

    pub async fn set_metadata(&self, ticket: &String, name: Option<String>, size: Option<usize>, compression: Option<Compression>) -> bool {
        match self.files.lock().await.get_mut(ticket) { // need mut just in case the upload is valid, so we can instantly lock it
            Some(meta) => {
                if name.is_some() {
                    meta.file_name = name.unwrap();
                }
                if size.is_some() {
                    meta.file_size = size.unwrap();
                }
                if compression.is_some() {
                    meta.compression = compression.unwrap();
                }
                true
            },
            None => false
        }
    }

    pub async fn end(&self, ticket: &String) -> bool {
        let mut meta = self.files.lock().await;

        match meta.get_mut(ticket) {
            Some(meta) => {
                    meta.end_download();
                    meta.end_upload();
                    true
                },
                None => false
        }
    }

    pub async fn end_upload(&self, ticket: &String) -> bool {
        let mut meta = self.files.lock().await;

        match meta.get_mut(ticket) {
            Some(meta) => {
                    meta.end_upload();
                    true
                },
                None => false
            }
    }

    // this really shouldn't be done unless doing cleanup, otherwise "end" is good enough
    pub async fn delete(&self, ticket: &String) -> bool {
        let mut meta = self.files.lock().await;

        if meta.contains_key(ticket) {
            meta.remove(ticket);
        } else {
            return false
        }
        let mut uploads = self.uploads.lock().await;
        let mut downloads = self.downloads.lock().await;

       uploads.remove(ticket);
       downloads.remove(ticket);

       true
    }

    pub async fn cull(&self) -> usize {
        std::thread::sleep(std::time::Duration::from_secs(10));
        trace!("Trying cull...");
        let meta = self.files.lock().await;
        let to_remove: Vec<String> = meta.keys() // need to deal with auth and not authed!
            .filter(|id| meta.get(*id).unwrap().age() > match meta.get(*id).unwrap().authenticated() {
                true => self.auth_options.get_cull_time(),
                false => self.reg_options.get_cull_time()
            })
            .filter(|id| meta.get(*id).unwrap().is_in_waiting_state()) // things that aren't waiting shouldn't be culled
            .cloned()
            .collect();

        trace!("Found {} items to cull", to_remove.len());
        drop(meta);
        // Then remove the IDs in a separate loop
        let rem = to_remove.len();
        for id in to_remove {
            self.delete(&id).await;
            debug!("Culled {}", id);
        }
        return rem;
    }
}
