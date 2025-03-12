use std::path::Path;
use clap::{Parser, Subcommand};
use client::{download::download_manager, upload::upload, ClientConfig, DownloadArgs, UploadArgs};
use serde::Deserialize;
use tracing::{error, Level};
use dotenv::dotenv;

mod utils; // this is needed in both server and client
mod client;

#[cfg(feature = "server")]
mod server;
#[cfg(feature = "server")]
use server::server::server;
#[cfg(feature = "server")]
use server::{ServerConfig, ServerArgs};

#[derive(Parser, Deserialize, Debug)]
#[command(name = "ByteBeam")]
#[command(version = env!("CARGO_PKG_VERSION"))]
#[command(propagate_version = true)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Sets a custom config file
    #[arg(short, long, value_name = "FILE", default_value = "~/.config/bytebeam.toml")]
    config: String,

    /// Turn debugging information on
    #[arg(short, long, default_value="info", env="LOGLEVEL")]
    loglevel: String
}

#[derive(Subcommand, Deserialize, Debug)]
enum Commands {
    #[cfg(feature = "server")]
    /// Runs the ByteBeam server
    Server(ServerArgs),
    
    /// Upload a file
    Up(UploadArgs),

    /// Download a file
    Down(DownloadArgs)
}

#[derive(Deserialize, Debug, Clone)]
struct Config {
    client: Option<ClientConfig>,

    #[cfg(feature = "server")]
    server: Option<ServerConfig>
}

#[tokio::main]
async fn main() {
    dotenv().ok();
    let cli: Cli = Cli::parse();

    let subscriber_level = match cli.loglevel.to_ascii_uppercase().as_str() {
        "TRACE" => Level::TRACE,
        "DEBUG" => Level::DEBUG,
        "INFO" => Level::INFO,
        "WARN" => Level::WARN,
        "ERROR" => Level::ERROR,
        _ => Level::INFO, // default if the environment variable is not set or invalid
    };

    tracing_subscriber::fmt().with_max_level(subscriber_level).init();

    // lets see if there's a config file
    let expanded = shellexpand::tilde(&cli.config).into_owned();
    let config_path = Path::new(&expanded);
    let config: Option<Config> = if config_path.exists() {
        // okay now we can try to parse it
         match toml::from_str(&std::fs::read_to_string(config_path).unwrap()) {
            Ok(c) => Some(c),
            Err(e) => {
                error!("Failed to parse config file: {:?}", e);
                None
            }  
        }
    } else {
        None
    };

    // You can check for the existence of subcommands, and if found use their
    // matches just as you would the top level cmd

    match cli.command {
        #[cfg(feature = "server")]
        Commands::Server (args)  => {
            let config = if let Some(kconfig) = config {
                if let Some(mut sconfig) = kconfig.server {
                     sconfig.apply_args(args);
                     sconfig
                } else {
                    ServerConfig::default()
                }
            } else {
                ServerConfig::default()
            };
            let _ = server(config).await;
        },

        Commands::Up (mut args) => {
            if let Some(kconfig) = config {
                if let Some(cconfig) = kconfig.client {
                    args.args.merge(cconfig);
                }
            }
            let _ = upload(args).await;
        },
        Commands::Down (mut args) => {
            if let Some(kconfig) = config {
                if let Some(cconfig) = kconfig.client {
                    args.args.merge(cconfig);
                }
            }
           let _ = download_manager(args).await;
        }
    }
}
