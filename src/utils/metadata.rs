use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
#[cfg(feature = "server")]
use rand::Rng;
#[cfg(feature = "server")]
use chrono::Duration;

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
    download: FileState,
    created: DateTime<Utc>,
    accessed: DateTime<Utc>
}

impl FileMetadata {
    #[cfg(feature = "server")]
    pub fn new() -> Self {
        FileMetadata {
            file_name: String::new(),
            file_size: 0,
            path: FileMetadata::get_secure_string(),
            upload_key: FileMetadata::get_secure_string(),
            upload: FileState::NotStarted,
            download: FileState::NotStarted,
            created: Utc::now(),
            accessed: Utc::now()
        }
    }

    pub fn get_upload_info(&self) -> (String, String) {
        (self.path.clone(), self.upload_key.clone())
    }

    pub fn upload_locked(&self) -> bool { // we cant really allow resumed uploads?
        return self.upload == FileState::InProgress || self.upload == FileState::Complete
    }

    pub fn download_finished(&self) -> bool {
        return self.download == FileState::Complete
    }

    pub fn get_token(&self) -> &String {
        &self.path
    }

    #[cfg(feature = "server")]
    pub fn check_key(&self, key: &String) -> bool {
        return self.upload_key == *key
    }

    #[cfg(feature = "server")]
    pub fn start_upload(&mut self, key: &String) -> bool {
        if !self.check_key(key) {
            return false;
        }
        self.upload = FileState::InProgress;
        true
    }

    #[cfg(feature = "server")]
    pub fn end_upload(&mut self) { // this is rather simple
        self.upload = FileState::Complete;
    }

    #[cfg(feature = "server")]
    pub fn start_download(&mut self) { // this is rather simple
        self.download = FileState::InProgress;
    }

    #[cfg(feature = "server")]
    pub fn pause_download(&mut self) {
        self.download = FileState::Paused;
    }

    #[cfg(feature = "server")]
    pub fn end_download(&mut self) { // this is rather simple
        self.download = FileState::Complete;
    }

    pub fn download_locked(&self) -> bool {
        return self.download == FileState::InProgress || self.download == FileState::Complete;
    }

    #[cfg(feature = "server")]
    pub fn download_pausable(&self) -> bool {
        return self.download == FileState::InProgress;
    }

    #[cfg(feature = "server")]
    pub fn redact(&self) -> Self {
        Self {
            file_name: "null".to_string(), // private to downloader
            upload_key: "null".to_string(), // private
            file_size: 0, // rather unknown during the download
            upload: self.upload.clone(),
            download: self.download.clone(),
            path: self.path.clone(),
            created: self.created.clone(),
            accessed: self.accessed.clone()
        }
    }

    #[cfg(feature = "server")]
    pub fn access(&mut self) {
        self.accessed = Utc::now();
    }

    #[cfg(feature = "server")]
    pub fn age(&self) -> Duration {
        Utc::now() - self.accessed
    }

    #[cfg(feature = "server")]
    pub fn is_in_waiting_state(&self) -> bool {
        self.download == FileState::NotStarted || self.upload == FileState::NotStarted
    }

    #[cfg(feature = "server")]
    fn get_secure_string() -> String {
        let mut rng = rand::rng();
        let words_raw = include_str!("../../wordlist.txt").trim(); // via https://gist.githubusercontent.com/dracos/dd0668f281e685bad51479e5acaadb93/raw/6bfa15d263d6d5b63840a8e5b64e04b382fdb079/valid-wordle-words.txt
        // now split by newlines
        let words = words_raw.split('\n').collect::<Vec<&str>>();

        let mut iter = vec![];

        for _ in 0..3 {
            iter.push(words[rng.random_range(0..words.len())].to_string());
        }

        return format!("{}-{}", rng.random_range(0..100), iter.join("-"));
    }
}