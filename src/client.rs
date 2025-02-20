use std::{path::PathBuf, sync::{Arc, Mutex}, time::Duration};

use anyhow::Result;
use async_stream::stream;
use bytesize::ByteSize;
use indicatif::{ProgressBar, ProgressStyle};
use reqwest::Body;
use tokio_util::io::ReaderStream;
use tracing::{debug, error, info};
use tokio_stream::StreamExt;

#[tokio::main]
pub async fn client(server: String, auth: String, filepath: PathBuf) -> Result<()> {

    // open file for streaming
    let file = tokio::fs::File::open(&filepath).await.unwrap();

    // get file length
    let file_len = file.metadata().await?.len();

    debug!("Found file length: {}", ByteSize(file_len).to_string_as(true));

    // get file name from path
    let file_name = std::path::Path::new(&filepath).file_name().unwrap_or_default().to_string_lossy();
    let mut reader_stream = ReaderStream::new(file);

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

    let encoded_file = urlencoding::encode(&file_name);
    let upload_path = format!("{server}/{encoded_file}");
    let send_path = match std::env::var("PROXIED_SERVER") {
        Ok(s) => format!("{s}/{encoded_file}"),
        Err(_) => upload_path.clone()
    };

    info!("Download available from: {send_path}");

    let client = reqwest::Client::new();
    let form = reqwest::multipart::Form::new()
        .text("authentication", auth)
        .text("file-size", file_len.to_string())
        .part("file", reqwest::multipart::Part::stream(Body::wrap_stream(async_stream)));

    let _ = client.post(&upload_path)
        .multipart(form)
        .send().await?;

    let fin_bytes = read_so_far.clone().lock().unwrap().clone();
    if fin_bytes == file_len {
        info!("File uploaded successfully.");
    } else {
        error!(
            "Client did not successfully download the whole file {}/{} ({}%)", 
            ByteSize(fin_bytes).to_string_as(true),
            ByteSize(file_len).to_string_as(true),
            ((fin_bytes as f64 / file_len as f64) * 100.0).round() as u8
        );
    }

    Ok(())
}
