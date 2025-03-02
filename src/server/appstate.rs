use std::{collections::HashMap, sync::Arc};
use tokio::sync::{mpsc::{channel, Receiver, Sender}, Mutex};
use tracing::{info, trace};

use crate::utils::metadata::FileMetadata;

#[derive(Debug, Clone)]
pub struct AppState {
    token: String,
    files: Arc<Mutex<HashMap<String, FileMetadata>>>,
    downloads: Arc<Mutex<HashMap<String, Receiver<Vec<u8>>>>>,
    uploads: Arc<Mutex<HashMap<String, Sender<Vec<u8>>>>>,
    cache_size: usize
}


impl AppState {
    pub fn new(token: &String, cache_size: usize) -> Self {
        Self {
            token: token.clone(),
            files: Arc::new(Mutex::new(HashMap::new())),
            downloads: Arc::new(Mutex::new(HashMap::new())),
            uploads: Arc::new(Mutex::new(HashMap::new())),
            cache_size
        }
    }

    pub async fn generate_file_upload(&self, token: &String, file_name: &String) -> Option<FileMetadata> {
        if *token != self.token {
            return None;
        }
        trace!("generate_file_upload getting elements");
        let mut uploads = self.uploads.lock().await;
        let mut downloads = self.downloads.lock().await;
        let mut meta = self.files.lock().await;
        trace!("Creating tx");
        let (tx, rx) = channel(self.cache_size);
        trace!("Creating new token for upload");
    
        let mut upload = FileMetadata::new();

        trace!("Setting file name");
        upload.file_name = file_name.clone();

        trace!("Inserting into uploads and downloads");
    
        uploads.insert(upload.get_token().clone(), tx);
        downloads.insert(upload.get_token().clone(), rx);

        meta.insert(upload.get_token().clone(), upload.clone());

        // we also need to loosely make sure the upload is authenticated, so upload should be tied to another key
        
        Some(upload)
    }

    pub async fn get_file_metadata(&self, ticket: &String) -> Option<FileMetadata> {
        let meta = self.files.lock().await;
        meta.get(ticket).cloned()
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
                    match self.uploads.lock().await.remove(ticket) {
                        Some(tx) => {
                            meta.start_upload(lock);
                            Some(tx) // yay!
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
}
