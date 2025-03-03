use serde::{Deserialize, Serialize};
use rand::Rng;
use tracing::trace;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum FileState {
    NotStarted,
    InProgress,
    Paused,
    Complete
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMetadata {
    pub file_name: String, // making getters/setters when nothing depends on this feels kinda useless
    pub file_size: usize,
    path: String,
    upload_key: String,
    upload: FileState,
    download: FileState
}

impl FileMetadata {
    pub fn new() -> Self {
        FileMetadata {
            file_name: String::new(),
            file_size: 0,
            path: FileMetadata::get_secure_string(),
            upload_key: FileMetadata::get_secure_string(),
            upload: FileState::NotStarted,
            download: FileState::NotStarted
        }
    }

    pub fn get_upload_info(&self) -> (String, String) {
        (self.path.clone(), self.upload_key.clone())
    }

    pub fn upload_locked(&self) -> bool { // we cant really allow resumed uploads?
        return self.upload == FileState::InProgress
    }

    pub fn get_token(&self) -> &String {
        &self.path
    }

    pub fn check_key(&self, key: &String) -> bool {
        return self.upload_key == *key
    }

    pub fn start_upload(&mut self, key: &String) -> bool {
        if !self.check_key(key) {
            return false;
        }
        self.upload = FileState::InProgress;
        true
    }

    pub fn start_download(&mut self) { // this is rather simple
        self.download = FileState::InProgress;
    }

    pub fn pause_download(&mut self) {
        self.download = FileState::Paused;
    }

    pub fn download_locked(&self) -> bool {
        return self.download == FileState::InProgress || self.download == FileState::Complete;
    }

    pub fn download_pausable(&self) -> bool {
        return self.download == FileState::InProgress;
    }

    fn get_secure_string() -> String {
        let mut rng = rand::rng();
        let words_raw = include_str!("../../wordlist.txt").trim(); // via https://gist.githubusercontent.com/dracos/dd0668f281e685bad51479e5acaadb93/raw/6bfa15d263d6d5b63840a8e5b64e04b382fdb079/valid-wordle-words.txt
        // now split by newlines
        let words = words_raw.split('\n').collect::<Vec<&str>>();

        let mut iter = vec![];

        for _ in 0..3 {
            iter.push(words[rng.random_range(0..words.len())].to_string());
        }

        trace!("{:?}", iter);

        return format!("{}-{}", rng.random_range(0..100), iter.join("-"));
    }
}