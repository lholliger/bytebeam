use std::path::PathBuf;
use clap::Args;
use serde::Deserialize;

pub mod upload;
pub mod download;
mod token;

#[derive(Args, Deserialize, Debug)]
pub struct UploadArgs {
    #[command(flatten)]
    pub args: ClientConfig,

    /// The token or URL to upload to, if not defined
    #[arg(short, long)]
    token: Option<String>,

    /// Optional filename to override for the upload
    #[arg(short, long)]
    name: Option<String>,

    /// the file to beam
    file: String,
}

impl UploadArgs {
    fn get_file_path(&self) -> PathBuf {
        let expanded = shellexpand::tilde(&self.file).into_owned();
        let p = PathBuf::new().join(expanded);
        p
    }
}

#[derive(Args, Deserialize, Debug)]
pub struct DownloadArgs {
    #[command(flatten)]
    pub args: ClientConfig,

    /// the output to write the file. If blank, will download to the upload name
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Overwrite if needed
    #[arg(short, long)]
    yes: bool,

    /// The URL/token to download. If blank, create a reverse-upload
    path: Option<String>,
}

#[derive(Args, Deserialize, Debug, Clone)]
pub struct ClientConfig {
    /// the ByteBeam server to connect to
    #[arg(short, long, value_name = "ADDRESS", env = "ADDRESS", default_value = "http://localhost:3000")]
    server: Option<String>,

    /// Username to authenticate against
    #[arg(short, long, default_value = "default")]
    username: Option<String>,

    /// Path for a key or keys to sign with
    #[arg(short, long, default_value = "~/.ssh")]
    key: Option<String>,
}

impl ClientConfig {
    pub fn merge(&mut self, config: ClientConfig) {
        match config.server {
            Some(server) => if server != "http://localhost:3000" {
                self.server = Some(server);
            },
            None => (),
        }

        match config.username {
            Some(username) => if username != "default" {
                self.username = Some(username);
            },
            None => (),
        }

        match config.key {
            Some(key) => if key != "~/.ssh" {
                self.key = Some(key);
            },
            None => (),
        }
    }

    pub fn get_absolute(&self) -> (String, String, String) {
        let server = match &self.server {
            Some(server) => server.clone(),
            None => "http://localhost:3000".to_string(),
        };
        let username = match &self.username {
            Some(username) => username.clone(),
            None => "default".to_string(),
        };
        let key = match &self.key {
            Some(key) => key.clone(),
            None => "~/.ssh".to_string(),
        };
        (server, username, key)
    }
}