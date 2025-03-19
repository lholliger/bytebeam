use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
#[cfg(feature = "server")]
use chrono::Duration;
#[cfg(feature = "server")]
use crate::server::serveropts::ServerOptions;


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
    accessed: DateTime<Utc>,
    authed_user: Option<String>,
    challenge: String, // this will generate a uuidv4 no matter what, if no authed_user is passed, it is rather useless
    authenticated: bool
}

impl FileMetadata {
    #[cfg(feature = "server")]
    pub fn new(options: &ServerOptions, user: Option<&String>) -> Self {
        use uuid::Uuid;

        FileMetadata {
            file_name: String::new(),
            file_size: 0,
            path: options.generate_upload_token(),
            upload_key: options.generate_key_token(),
            upload: FileState::NotStarted,
            download: FileState::NotStarted,
            created: Utc::now(),
            accessed: Utc::now(),
            authed_user: match user {
                Some(u) => Some(u.clone()),
                None => None,
            },
            challenge: format!("{}", Uuid::new_v4()),
            authenticated: false
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
            upload_key: "null".to_string(), // defeats the purpose of having this path
            file_size: 0, // rather unknown during the download
            upload: self.upload.clone(),
            download: self.download.clone(),
            path: self.path.clone(),
            created: self.created.clone(),
            accessed: self.accessed.clone(),
            authed_user: self.authed_user.clone(), // maybe should be private?
            challenge: self.challenge.clone(),
            authenticated: self.authenticated
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

    pub fn authenticated(&self) -> bool {
        self.authenticated
    }

    pub fn get_challenge_details(&self) -> Option<(bool, &String, &String)> {
        match &self.authed_user {
            Some(user) => {
                Some((self.authenticated(), user, &self.challenge))
            },
            None => None
        }
    }

    #[cfg(feature = "server")]
    pub fn upgrade(&mut self, options: &ServerOptions) { // TODO: if the token formats are the same, don't change the key
            self.authenticated = true;
            self.path = options.generate_upload_token();
            self.upload_key = options.generate_key_token();
            self.accessed = Utc::now();
    }
}