use std::{collections::HashMap, sync::Arc, thread};
use chrono::Duration;
use tokio::sync::{mpsc::{channel, Receiver, Sender}, Mutex};
use tracing::{debug, info, trace};

use crate::utils::metadata::FileMetadata;

#[derive(Debug, Clone)]
pub struct AppState {
    token: String,
    files: Arc<Mutex<HashMap<String, FileMetadata>>>,
    downloads: Arc<Mutex<HashMap<String, Receiver<Vec<u8>>>>>,
    uploads: Arc<Mutex<HashMap<String, Sender<Vec<u8>>>>>,
    cache_size: usize,
    pub block_size: usize,
    cull_time: Duration
}


impl AppState {
    pub fn new(token: &String, cache_size: usize) -> Self {
        let state = AppState {
            token: token.clone(),
            files: Arc::new(Mutex::new(HashMap::new())),
            downloads: Arc::new(Mutex::new(HashMap::new())),
            uploads: Arc::new(Mutex::new(HashMap::new())),
            cache_size,
            block_size: 4096,
            cull_time: Duration::hours(1)
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

    pub async fn generate_file_upload(&self, token: &String, file_name: &String) -> Option<FileMetadata> {
        if *token != self.token {
            return None;
        }
        let mut uploads = self.uploads.lock().await;
        let mut downloads = self.downloads.lock().await;
        let mut meta = self.files.lock().await;
        let (tx, rx) = channel(self.cache_size);
    
        let mut upload = FileMetadata::new();

        upload.file_name = file_name.clone();
    
        uploads.insert(upload.get_token().clone(), tx);
        downloads.insert(upload.get_token().clone(), rx);

        meta.insert(upload.get_token().clone(), upload.clone());

        // we also need to loosely make sure the upload is authenticated, so upload should be tied to another key
        
        Some(upload)
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
    pub async fn begin_upload(&self, ticket: &String, lock: &String) -> Option<Sender<Vec<u8>>> {
        match self.files.lock().await.get_mut(ticket) { // need mut just in case the upload is valid, so we can instantly lock it
            Some(meta) => {
                if meta.upload_locked() { // cannot allow another upload
                    None
                } else if !meta.check_key(lock) { // user didnt use the right key!
                        return None;
                } else {
                    // okay, we've verified the upload so now we can lock it
                    match self.uploads.lock().await.get(ticket) {
                        Some(tx) => {
                            meta.start_upload(lock);
                            Some(tx.clone()) // yay!
                        },
                        None => None
                    }
                }
            },
            None => None
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

    pub async fn set_metadata(&self, ticket: &String, name: Option<String>, size: Option<usize>) -> bool {
        match self.files.lock().await.get_mut(ticket) { // need mut just in case the upload is valid, so we can instantly lock it
            Some(meta) => {
                if name.is_some() {
                    meta.file_name = name.unwrap();
                }
                if size.is_some() {
                    meta.file_size = size.unwrap();
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

    pub async fn cull(&self) -> usize{
        std::thread::sleep(std::time::Duration::from_secs(10));
        trace!("Trying cull...");
        let meta = self.files.lock().await;
        let to_remove: Vec<String> = meta.keys()
            .filter(|id| meta.get(*id).unwrap().age() > self.cull_time)
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
