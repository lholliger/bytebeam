use std::{path::PathBuf, sync::{Arc, Mutex}, thread, time::Duration};
use async_stream::stream;
use bytes::Bytes;
use bytesize::ByteSize;
use indicatif::{ProgressBar, ProgressStyle};
use reqwest::Body;
use tokio::io;
use tokio_util::io::ReaderStream;
use tracing::{debug, error, warn};
use tokio_stream::{Stream, StreamExt};
use url::Url;

use crate::{client::token::get_upload_token, utils::metadata::FileMetadata};

pub async fn upload(server: String, username: String, filepath: PathBuf, token: Option<String>, name_override: Option<String>) -> Result<(), ()> {

    let mut file_name = "bytebeam".to_string();
    let mut file_len = 0;

    let mut reader_stream = if !filepath.exists() {
        let filepath_str = filepath.to_str().expect("Could not convert path to string");
        if filepath_str == "-" {
            if name_override.is_none() {
                warn!("No file name specified. Defaulting to \"bytebeam\". This can be defined using --name [FILENAME]");
            }
            debug!("Reading from stdin...");
            Box::new(ReaderStream::new(Box::new(tokio::io::stdin()))) as Box<dyn Stream<Item = Result<Bytes, io::Error>> + Unpin + Send>
        } else {
            error!("Path does not exist: {}", filepath_str);
            return Err(());
        }
    } else {
        let file = tokio::fs::File::open(&filepath).await.unwrap();
        file_len = file.metadata().await.expect("Could not read metadata").len();
        debug!("Found file length: {}", ByteSize(file_len).to_string_as(true));
        file_name = std::path::Path::new(&filepath).file_name().unwrap_or_default().to_string_lossy().to_string();
        
        Box::new(ReaderStream::new(file)) as Box<dyn Stream<Item = Result<Bytes, io::Error>> + Unpin + Send>
    };



    // if we already have a token, we can skip much of the next part

    let mut thread: Option<std::thread::JoinHandle<()>> = None;

    let upload_path = match token {
        Some(tok) => {
            match Url::parse(&tok) {
                Ok(u) => u,
                Err(_) => match Url::parse(format!("{server}/{tok}").as_str()) {
                    Ok(u) => u,
                    Err(_) => {
                        error!("Invalid upload URL: {}", tok);
                        return Err(());
                    },
                }
            }
        },
        None => {
            let encoded_file = match name_override {
                Some(name) => urlencoding::encode(&name).to_string(),
                None => urlencoding::encode(&file_name).to_string(),
            };

            let upload_path = format!("{server}/{encoded_file}");
        
            // so we need to get the download
        
            let metadata = match get_upload_token(&username, file_len as usize, upload_path).await {
                Some(metadata) => metadata,
                None => {
                    error!("Failed to get upload token");
                    return Err(());
                }
            };

            if username != "default".to_string() {

            }
        
            let ul = metadata.get_upload_info();
            let upload_path = match Url::parse(format!("{server}/{}/{}", ul.0, ul.1).as_str()) {
                Ok(u) => u,
                Err(e) => {
                    error!("Invalid URL, is the server correct? {:?}", e);
                    return Err(());
                }
            };
            let check_url = format!("{server}/{}?status=true", ul.0);

            let send_path = match std::env::var("PROXIED_SERVER") {
                Ok(s) => format!("{s}/{}", ul.0),
                Err(_) => format!("{server}/{}", ul.0)
            };

            qr2term::print_qr(&send_path).expect("Could not generate QR code");
            println!("\nDownload is available from: {}\n\n", send_path);

            // we need to keepalive!
            thread = Some(thread::spawn(move || {
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.block_on(async {
                    let mut is_downloading = false;
                    loop {
                        let status = match reqwest::get(&check_url).await {
                            Ok(req) => req,
                            Err(e) => {
                                error!("Failed to connect to server for status: {}", e);
                                break;
                            }
                        };
                
                        match status.json::<FileMetadata>().await {
                            Ok(meta) => {
                                if meta.download_locked() && !is_downloading {
                                    println!("Client has begun downloading!");
                                    is_downloading = true;
                                }
                                if meta.download_finished() {
                                    println!("done!");
                                    break;
                                }
                            }
                            Err(e) => {
                                error!("Failed to parse download metadata. Was the upload deleted? {:?}", e);
                                break;
                            }
                        }
                        std::thread::sleep(std::time::Duration::from_secs(10));
                    }
                });
            }));


            upload_path
        }
    };
    // okay, now we just upload

    let bar = ProgressBar::new(file_len as u64);
    bar.set_style(ProgressStyle::with_template("[{elapsed_precise}] {bar:40.cyan/blue} {bytes:>7}/{total_bytes:7} {msg}")
        .unwrap());
    bar.enable_steady_tick(Duration::from_millis(100));
    let read_so_far: Arc<Mutex<u64>> = Arc::new(Mutex::new(0));

    let int_read = read_so_far.clone();
    let async_stream = stream! {
        while let Some(chunk) = reader_stream.next().await {
            if let Ok(chunk) = &chunk {
                let mut b = int_read.lock().unwrap();
                *b += chunk.len() as u64;
                bar.set_position(*b);
            }
            yield chunk;
        }
    };
    
    let client = reqwest::Client::new();
    let form = reqwest::multipart::Form::new()
        .text("file-size", file_len.to_string())
        .part("file", reqwest::multipart::Part::stream(Body::wrap_stream(async_stream)));

    match client.post(upload_path)
        .multipart(form)
        .send().await {
            Ok(response) => {
                if !response.status().is_success() {
                    error!(
                        "Non-success response from Beam server: {}",
                        response.text().await.unwrap()
                    );
                }
            },
            Err(e) => {
                error!("Failed to connect to Beam server: {}", e);
            }
        }

    let fin_bytes = read_so_far.clone().lock().unwrap().clone();
    if fin_bytes == file_len {
        println!("File uploaded successfully. ({} bytes)", &fin_bytes);
    } else if fin_bytes > file_len {
        // TODO: we should update the total
    } else {
        error!(
            "Client did not successfully download the whole file {}/{} ({}%)", 
            ByteSize(fin_bytes).to_string_as(true),
            ByteSize(file_len).to_string_as(true),
            ((fin_bytes as f64 / file_len as f64) * 100.0).round() as u8
        );
    }

    match thread {
        Some(thread) => {
            println!("Waiting for client to download...");
            thread.join().unwrap();
        },
        None => {}
    }

    Ok(())
}
