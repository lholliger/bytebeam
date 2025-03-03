use std::path::PathBuf;
use clap::{Parser, Subcommand, Args};
use client::{download::download_manager, upload::upload};
use serde::Deserialize;

use tracing::Level;
use dotenv::dotenv;

mod utils; // this is needed in both server and client
mod client;

#[cfg(feature = "server")]
mod server;
#[cfg(feature = "server")]
use server::server::server;

#[derive(Parser)]
#[command(name = "ByteBeam")]
#[command(version = "0.1.0")]
#[command(propagate_version = true)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Sets a custom config file
    #[arg(short, long, value_name = "FILE", default_value = "~/.config/bytebeam.toml")]
    config: Option<PathBuf>,

    /// Turn debugging information on
    #[arg(short, long, default_value="info", env="LOGLEVEL")]
    loglevel: String,

    /// authentication string
    #[arg(short, long, value_name = "TOKEN", default_value = "password", env="AUTH")]
    auth: String
}

#[derive(Subcommand)]
enum Commands {
    #[cfg(feature = "server")]
    /// Runs the ByteBeam server
    Server(ServerArgs),
    
    /// Upload a file
    Up(UploadArgs),

    /// Download a file
    Down(DownloadArgs)
}

#[derive(Args)]
#[cfg(feature = "server")]
struct ServerArgs {
    /// the address to listen on
    #[arg(long, value_name = "ADDRESS", default_value = "0.0.0.0:3000", env="LISTEN")]
    listen: String,

    /// the size, in bytes, to cache each file in memory for read
    #[arg(short, long, value_name = "BYTES", default_value = "1073741824", env="CACHE_SIZE")]
    cache: usize
}

#[derive(Args, Deserialize)]
struct UploadArgs {
    /// the ByteBeam server to connect to
    #[arg(short, long, value_name = "ADDRESS", default_value = "http://localhost:3000")]
    server: String,

    /// The token or URL to upload to, if not defined
    #[arg(short, long)]
    token: Option<String>,

    /// the file to beam
    file: String,
}

#[derive(Args, Deserialize)]
struct DownloadArgs {
    /// the ByteBeam server to connect to
    #[arg(short, long, value_name = "ADDRESS", default_value = "http://localhost:3000")]
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


    // You can check for the existence of subcommands, and if found use their
    // matches just as you would the top level cmd
    match &cli.command {
        #[cfg(feature = "server")]
        Commands::Server (args)  => {
            let _ = server(args.listen.clone(), args.cache, cli.auth.clone()).await;
        },

        Commands::Up (args) => {
            let _ = upload(args.server.clone(), cli.auth.clone(), args.file.clone().into(), args.token.clone()).await;
        },
        Commands::Down (args) => {
           let _ = download_manager(args.server.clone(), cli.auth.clone(), args.output.clone(), args.path.clone(), args.yes).await;
        }
    }
}
