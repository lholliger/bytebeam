use std::path::PathBuf;
use clap::{Parser, Subcommand, Args};
use serde::Deserialize;
use server::server::server;
use tracing::{error, Level};
use dotenv::dotenv;

mod utils; // this is needed in both server and client
mod server;
mod client;

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
    /// Runs the ByteBeam server
    Server(ServerArgs),
    
    /// Upload a file
    Up(ClientArgs),

    /// Download a file
    Down(ClientArgs)
}

#[derive(Args)]
struct ServerArgs {
    /// the address to listen on
    #[arg(long, value_name = "ADDRESS", default_value = "0.0.0.0:3000", env="LISTEN")]
    listen: String,

    /// the size, in bytes, to cache each file in memory for read
    #[arg(short, long, value_name = "BYTES", default_value = "1073741824", env="CACHE_SIZE")]
    cache: usize
}

#[derive(Args, Deserialize)]
struct ClientArgs {
    /// the ByteBeam server to connect to
    #[arg(short, long, value_name = "ADDRESS", default_value = "http://localhost:3000")]
    server: String,

    /// the file to beam
    file: PathBuf,
}

fn main() {
    dotenv().ok();
    let cli = Cli::parse();

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
        Commands::Server (args)  => {
            // TODO: actually handle exit cases
            let _ = server(args.listen.clone(), args.cache, cli.auth.clone());
        },
        Commands::Up (args) => {
            let _ = client::client(args.server.clone(), cli.auth.clone(), args.file.clone());
        }

        Commands::Down (_) => { // if no option given, create a download link, if an option is given, download. Could also be an external URL
            error!("Download not implemented yet");
        }
    }
}
