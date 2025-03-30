use std::{sync::{Arc, Mutex}, thread, time::Duration};
use bytes::Bytes;
use bytesize::ByteSize;
use indicatif::{ProgressBar, ProgressStyle};
use reqwest::Body;
use tokio::io;
use tokio_util::io::ReaderStream;
use tracing::{debug, error, warn};
use tokio_stream::Stream;
use url::Url;

use crate::{client::token::{do_run_upgrade_on_metadata, get_upload_token}, utils::{compression::Compression, metadata::FileMetadata}};

use super::{compression::ProgressStream, UploadArgs};

pub async fn upload(config: UploadArgs) -> Result<(), ()> {
    let filepath = config.get_file_path();
    let (server, username, key) = config.args.get_absolute();

    let token = config.token;

    let mut file_name = "bytebeam".to_string();
    let mut file_len = 0;

    let reader_stream = if !filepath.exists() {
        let filepath_str = filepath.to_str().expect("Could not convert path to string");
        if filepath_str == "-" {
            if config.name.is_none() {
                warn!("No file name specified. Defaulting to \"bytebeam\". This can be defined using --name [FILENAME]");
            }
            debug!("Reading from stdin...");
            Box::new(ReaderStream::new(Box::new(tokio::io::stdin()))) as Box<dyn Stream<Item = Result<Bytes, io::Error>> + Unpin + Send>
        } else {
            error!("Path does not exist: {}", filepath_str);
            return Err(());
        }
    } else {
        // see if file is a folder, so we need to send the whole thing
        if filepath.is_dir() {
            //let mut file_list = tokio::fs::read_dir(&filepath).await.unwrap();

            error!("Folder support is not ready yet");
            return Err(());
        } else {
            let file = tokio::fs::File::open(&filepath).await.unwrap();
            file_len = file.metadata().await.expect("Could not read metadata").len();
            debug!("Found file length: {}", ByteSize(file_len).to_string_as(true));
            file_name = std::path::Path::new(&filepath).file_name().unwrap_or_default().to_string_lossy().to_string();
            
            Box::new(ReaderStream::new(file)) as Box<dyn Stream<Item = Result<Bytes, io::Error>> + Unpin + Send>
        }
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
            let encoded_file = match config.name {
                Some(name) => urlencoding::encode(&name).to_string(),
                None => urlencoding::encode(&file_name).to_string(),
            };

            let upload_path = format!("{server}/{encoded_file}");
        
            // so we need to get the download
        
            let metadata = match get_upload_token(&username, file_len as usize, upload_path).await {
                Some(metadata) => do_run_upgrade_on_metadata(metadata, &username, &key, &server).await,
                None => {
                    error!("Failed to get upload token");
                    return Err(());
                }
            };
        
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
                        if is_downloading {
                            std::thread::sleep(std::time::Duration::from_secs(5));
                        } else {
                            std::thread::sleep(std::time::Duration::from_secs(10));

                        }
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

    let progress_stream = ProgressStream::new(
        reader_stream,
        read_so_far.clone(),
        bar.clone(),
        config.compression.clone()
    );

    let async_stream = progress_stream.into_stream();
    
    
    let client = reqwest::Client::new();
    let form = reqwest::multipart::Form::new()

        .text("file-size", match config.compression { // output size changes
            Compression::None => file_len.to_string(),
            _ => "0".to_string()
        })
        .text("compression", config.compression.to_string())
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
