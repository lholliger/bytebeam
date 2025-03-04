use std::path::{Path, PathBuf};
use clap::{Parser, Subcommand, Args};
use client::{download::download_manager, upload::upload};
use serde::Deserialize;

use tracing::{error, trace, Level};
use dotenv::dotenv;

mod utils; // this is needed in both server and client
mod client;

#[cfg(feature = "server")]
mod server;
#[cfg(feature = "server")]
use server::server::server;

#[derive(Parser, Deserialize, Debug)]
#[command(name = "ByteBeam")]
#[command(version = "0.1.0")]
#[command(propagate_version = true)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Sets a custom config file
    #[arg(short, long, value_name = "FILE", default_value = "~/.config/bytebeam.toml")]
    config: String,

    /// Turn debugging information on
    #[arg(short, long, default_value="info", env="LOGLEVEL")]
    loglevel: String,

    /// authentication string
    #[arg(short, long, value_name = "TOKEN", default_value = "password", env="AUTH")]
    auth: String
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

#[derive(Args, Deserialize, Debug)]
#[cfg(feature = "server")]
struct ServerArgs {
    /// the address to listen on
    #[arg(long, value_name = "ADDRESS", default_value = "0.0.0.0:3000", env="LISTEN")]
    listen: String,

    /// the size, in bytes, to cache each file in memory for read
    #[arg(short, long, value_name = "BYTES", default_value = "1073741824", env="CACHE_SIZE")]
    cache: usize
}

#[derive(Args, Deserialize, Debug)]
struct UploadArgs {
    /// the ByteBeam server to connect to
    #[arg(short, long, value_name = "ADDRESS", env = "ADDRESS", default_value = "http://localhost:3000")]
    server: String,

    /// The token or URL to upload to, if not defined
    #[arg(short, long)]
    token: Option<String>,

    /// the file to beam
    file: String,
}

#[derive(Args, Deserialize, Debug)]
struct DownloadArgs {
    /// the ByteBeam server to connect to
    #[arg(short, long, value_name = "ADDRESS", env = "ADDRESS", default_value = "http://localhost:3000")]
    server: String,

    /// the output to write the file. If blank, will download to the upload name
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// The URL/token to download. If blank, create a reverse-upload
    path: Option<String>,

    /// Overwrite if needed
    #[arg(short, long)]
    yes: bool
}

#[derive(Deserialize, Debug, Clone)]
struct Config {
    auth: Option<String>,
    client: Option<ClientConfig>,

    #[cfg(feature = "server")]
    server: Option<ServerConfig>
}

#[cfg(feature = "server")]
#[derive(Deserialize, Debug, Clone)]
struct ServerConfig {
    listen: Option<String>,
    cache: Option<usize>
}

#[derive(Deserialize, Debug, Clone)]
struct ClientConfig {
    server: Option<String>,
}

#[tokio::main]
async fn main() {
    dotenv().ok();
    let mut cli: Cli = Cli::parse();

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

    match config.clone() {
        Some(c) => match c.auth {
            Some(a) => {
                trace!("Auth set using config");
                cli.auth = a
            },
            None => ()
        }
        None => ()
    };

    // You can check for the existence of subcommands, and if found use their
    // matches just as you would the top level cmd

    match cli.command {
        #[cfg(feature = "server")]
        Commands::Server (mut args)  => {
            if config.is_some() {
                let cs = config.unwrap();
                if cs.server.is_some() {
                    let server_args = cs.server.unwrap();
                    if server_args.listen.is_some() {
                        args.listen = server_args.listen.unwrap();
                        trace!("Using config server listen: {}", args.listen);
                    }
                    if server_args.cache.is_some() {
                        args.cache = server_args.cache.unwrap();
                        trace!("Using config server cache: {}", args.cache);
                    }
                }
            }
            let _ = server(args.listen.clone(), args.cache, cli.auth.clone()).await;
        },

        Commands::Up (mut args) => {
            if config.is_some() {
                let cs: Config = config.unwrap();
                if cs.client.is_some() {
                    let c_args = cs.client.unwrap();
                    if c_args.server.is_some() {
                        args.server = c_args.server.unwrap();
                        trace!("Using config server: {}", args.server);
                    }
                }
            }
            let _ = upload(args.server.clone(), cli.auth.clone(), args.file.clone().into(), args.token.clone()).await;
        },
        Commands::Down (mut args) => {
            if config.is_some() { // TODO: dont duplicate code here
                let cs = config.unwrap();
                if cs.client.is_some() {
                    let c_args = cs.client.unwrap();
                    if c_args.server.is_some() {
                        args.server = c_args.server.unwrap();
                        trace!("Using config server: {}", args.server);
                    }
                }
            }
           let _ = download_manager(args.server.clone(), cli.auth.clone(), args.output.clone(), args.path.clone(), args.yes).await;
        }
    }
}
