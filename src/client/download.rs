use std::{io, io::Write, path::PathBuf, time::Duration};

use indicatif::{ProgressBar, ProgressStyle};
use tokio::fs::File;
use tracing::error;
use url::Url;
use urlencoding::decode;
use tokio_stream::StreamExt;
use tokio::io::AsyncWriteExt;

use crate::utils::metadata::FileMetadata;

use super::token::get_upload_token;

pub async fn download_manager(server: String, username: String, output: Option<PathBuf>, input: Option<String>, yes: bool) -> Result<(), ()> {
    let download_path = match input {
        Some(piece) => {
            let url = match Url::parse(&piece) {
                Ok(url) => url,
                Err(_) => match Url::parse(format!("{server}/{piece}").as_str()) {
                    Ok(url) => url,
                    Err(_) => {
                        error!("Invalid URL provided: {}", piece);
                        return Err(());
                    }
                }
            };

            // now we can just run the download
            url
        },
        None => {
            if output.is_none() {
                error!("No input or output provided. Please provide a Beam code and/or a path to download to.");
                return Err(());
            }
            // this is weird since a filename needs to be provided, as its defined here
            let op = output.clone().unwrap();
            let file_name = std::path::Path::new(&op).file_name().unwrap_or_default().to_string_lossy();
            let encoded_file = urlencoding::encode(&file_name);
            let download_path = format!("{server}/{encoded_file}");

            match get_upload_token(&username, 0, download_path).await {
                Some(meta) => {
                    let download_path = format!("{server}/{}", meta.get_token());
                    match Url::parse(&download_path) {
                        Ok(url) => {
                            let upload_info = meta.get_upload_info();
                            let upload_path = format!("{server}/{}/{}", upload_info.0, upload_info.1);
                            qr2term::print_qr(&upload_path).expect("Could not generate QR code");

                            println!("\nUpload is available from: {}\n\n", upload_path);

                            // include some things about how to curl upload here
                            url
                        },
                        Err(_) => {
                            error!("Got token, but could not parse URL for {download_path}");
                            return Err(());
                        }
                    }
                },
                None => {
                    error!("Failed to get upload token. Please check your authentication and try again.");
                    return Err(());
                }
            }

            // we can give the user the path to download to, as well as some curl commands
        }
    };

    // we should wait until we can verify the metadata
    println!("Waiting for download...");
    loop {
        let status = match reqwest::get(format!("{download_path}?status=true")).await {
            Ok(req) => req,
            Err(e) => {
                error!("Failed to connect to server for status: {}", e);
                return Err(());
            }
        };

        match status.json::<FileMetadata>().await {
            Ok(meta) => {
                if !meta.download_locked() && meta.upload_locked() {
                    println!("Download is ready!");
                    break;
                }
            }
            Err(e) => {
                error!("Failed to parse download metadata: {}", e);
                return Err(());
            }
        }
        print!(".");
        std::thread::sleep(std::time::Duration::from_secs(15));
    }
    println!("download ready");

    // okay, now we can just download


    let request = match reqwest::get(download_path).await {
        Ok(req) => req,
        Err(e) => {
            error!("Failed to connect to server: {}", e);
            return Err(());
        }
    };

    if request.status() != reqwest::StatusCode::OK {
        error!("Failed to download file: {}", request.status().to_string());
        error!("Response: {}", request.text().await.expect("Could not get response"));
        return Err(());
    }

    // can we get the file name?

    let write_path = match output {
        Some(op) => op,
        None => {
            match request.url().path_segments().and_then(|segments| segments.last()) {
                Some(name) => match decode(name) {
                    Ok(name) => name.into_owned().into(),
                    Err(e) => {
                        error!("Failed to decode file name from request url: {:?}", e);
                        return Err(());
                    }
                },
                None => {
                    error!("Could not determine file name to save to, and none was provided. Cancelling download");
                    return Err(());
                }
            }
        }
    };

    if write_path.exists() && !yes {
        print!("File already exists: {:?}. Overwrite? [y/N] ", write_path);
        io::stdout().flush().expect("Could not flush stdout");
        
        let mut input = String::new();
        io::stdin().read_line(&mut input).expect("Could not read input");
        
        if !input.trim().eq_ignore_ascii_case("y") {
            error!("Download cancelled - file exists");
            return Err(());
        }
    }


    let mut file = match File::create(&write_path).await {
        Ok(file) => file,
        Err(e) => {
            error!("Failed to create output file: {}", e);
            return Err(());
        }
    };

    println!("Downloading to {:?}", write_path);

    let content_length = request
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);

    let bar = ProgressBar::new(content_length);
    bar.set_style(ProgressStyle::with_template("[{elapsed_precise}] {bar:40.cyan/blue} {bytes:>7}/{total_bytes:7} {msg}")
        .unwrap());
    bar.enable_steady_tick(Duration::from_millis(100));

    let mut stream = request.bytes_stream();
    while let Some(chunk_result) = stream.next().await {
        match chunk_result {
            Ok(chunk) => {
                    bar.inc(chunk.len() as u64);
                    match file.write_all(&chunk).await {
                    Ok(_) => (),
                    Err(e) => {
                        error!("Failed to write data to output file: {}", e);
                        return Err(());
                    }
                }
            }
            Err(e) => {
                error!("Failed to decode chunk: {}", e);
                return Err(());
            }
        }
    }

    bar.finish();

    println!("Download complete.");

    Ok(())
}