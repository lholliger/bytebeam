use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use super::compression::Compression;
#[cfg(feature = "server")]
use tracing::warn;
#[cfg(feature = "server")]
use bytesize::ByteSize;
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
    pub file_size: FileSize,
    compression: Compression,
    path: String,
    upload_key: String,
    upload: FileState,
    download: FileState,
    created: DateTime<Utc>,
    accessed: DateTime<Utc>,
    authed_user: Option<String>,
    challenge: String, // this will generate a uuidv4 no matter what, if no authed_user is passed, it is rather useless
    authenticated: bool,
}

impl FileMetadata {
    #[cfg(feature = "server")]
    pub fn new(options: &ServerOptions, user: Option<&String>) -> Self {
        use uuid::Uuid;

        FileMetadata {
            file_name: String::new(),
            file_size: FileSize::new(true),
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
            authenticated: false,
            compression: Compression::default()
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
            file_size: self.file_size.clone(), // should this need to be authenticated? Should there be a metadata key?
            upload: self.upload.clone(),
            download: self.download.clone(),
            path: self.path.clone(),
            created: self.created.clone(),
            accessed: self.accessed.clone(),
            authed_user: self.authed_user.clone(), // maybe should be private?
            challenge: self.challenge.clone(),
            authenticated: self.authenticated,
            compression: self.compression.clone(),
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

    #[cfg(feature = "server")]
    pub fn set_compression(&mut self, compression: Compression) {
        self.compression = compression;
        if self.compression != Compression::None {
            self.file_size.set_trustworthiness(false);
        } else {
            self.file_size.set_trustworthiness(true);
        }
    }

    pub fn get_compression(&self) -> Compression {
        self.compression.clone()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileSize {
    file_size: Option<usize>, // raw file size as reported by beam up, pre-compression
    uploaded_size: usize, // total number of bytes uploaded, will be post-compression. This value is constantly increasing. Since this does streaming, this value may never be complete if the file is over the cache size
    downloaded_size: usize, // download progress, will need to be equal to uploaded size at completion
    upload_complete: bool, // this is to know id uploaded_size is to be trusted
    file_size_trustworthy: bool
    // file_size is only sent as header when there is no compression, when upload_complete is true, uploaded_size will be defined as the header
}

#[cfg(feature = "server")]
impl FileSize {
    pub fn new(trusted: bool) -> Self {
        Self { 
            file_size: None,
            uploaded_size: 0,
            downloaded_size: 0,
            upload_complete: false,
            file_size_trustworthy: trusted
        }
    }
    pub fn set_file_size(&mut self, size: usize) {
        self.file_size = Some(size);
    }

    pub fn get_content_length(&self) -> Option<usize> {
        if self.file_size_trustworthy { // this would happen when there's no compression
            self.file_size
        } else if self.upload_complete { // this happens when the upload is complete so the compressed size is accurate
            Some(self.uploaded_size)
        } else { // it is still streaming in and isn't known yet
            None
        }
    }

    pub fn increase_upload(&mut self, size: usize) {
        self.uploaded_size += size;
    }

    pub fn get_uploaded_size(&self) -> usize {
        self.uploaded_size
    }

    pub fn increase_download(&mut self, size: usize) {
        self.downloaded_size += size;
        if self.downloaded_size > self.uploaded_size {
            warn!("Download progress is larger than upload size. This should not happen {} vs {}", self.downloaded_size, self.uploaded_size);
        }
    }

    pub fn get_download_progress(&self) -> usize {
        self.downloaded_size
    }

    fn set_trustworthiness(&mut self, trusted: bool) {
        self.file_size_trustworthy = trusted;
    }

    pub fn download_complete(&self) -> bool {
        self.upload_complete
    }

    pub fn get_file_string(&self) -> String {
        if self.file_size_trustworthy {
            if let Some(size) = self.file_size {
                return format!("{} ({} bytes)", ByteSize(size as u64).to_string_as(true), (size));
            }
        }
        return format!("Unknown");
    }
}