use serde::Deserialize;
use clap::Args;
use serveropts::ServerOptions;
use tracing::warn;
mod appstate;
pub mod server;
pub mod serveropts;
pub mod keymanager;

#[derive(Args, Deserialize, Debug)]
pub struct ServerArgs {
    /// the address to listen on
    #[arg(long, value_name = "ADDRESS", env="LISTEN")]
    listen: Option<String>,

    #[arg(long, value_name = "KEYSERVER", env="KEYSERVER")]
    keyserver: Option<String>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct ServerConfig {
    listen: Option<String>,
    public_options: Option<ServerOptions>,
    authenticated_options: Option<ServerOptions>,
    keyserver: Option<String>,
    users: Vec<String>
}

impl ServerConfig {
    pub fn default() -> Self {
        ServerConfig {
            listen: None,
            public_options: None,
            authenticated_options: None,
            keyserver: None,
            users: Vec::new()
        }
    }
    pub fn apply_args(&mut self, args: ServerArgs) {
       self.listen = Some(match args.listen {
            Some(l) => l,
            None => {
                warn!("Server not provided. Using default!");
                "0.0.0.0:3000".to_string()
            }
        });

        self.keyserver = match args.keyserver {
            Some(k) => Some(k),
            None => {
                warn!("Key server not provided. Authentication will not be possible without defined keys or a keyserver!");
                None
            }
        };
    }
}