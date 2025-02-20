use std::{collections::HashMap, sync::Arc};
use anyhow::Result;
use async_stream::stream;
use axum::{body::Body, extract::{DefaultBodyLimit, Multipart, Path, Query, State}, http::{HeaderMap, Response, StatusCode}, response::{IntoResponse, Redirect}, routing::{get, post}, Router};
use bytesize::ByteSize;
use chrono::Utc;
use maud::{html, Markup};
use sha2::{Digest, Sha256};
use tracing::{debug, error, info, warn, trace};
use tokio::sync::{mpsc::{channel, Receiver}, Mutex};


#[derive(Debug, Clone)]
struct AppState {
    token: String,
    files: Arc<Mutex<HashMap<String, (usize, Receiver<Vec<u8>>)>>>,
    downloads: Arc<Mutex<HashMap<(String, String), (usize, Arc<Mutex<Receiver<Vec<u8>>>>)>>>,
    cache_size: usize
}

#[tokio::main]
pub async fn server(address: String, data_storage: usize, token: String) -> Result<()> {
    const CHUNK_SIZE: usize = 4096; // this is being assumed, it shouldn't be
    let cache_size: usize = data_storage / CHUNK_SIZE;

    if token == "password" {
        warn!("WARNING - Using the default password is not recommended. Please use a secure token.")
    }

    let state = AppState {
        token,
        files: Arc::new(Mutex::new(HashMap::new())),
        downloads: Arc::new(Mutex::new(HashMap::new())),
        cache_size
    };

    info!("Starting server listening on {}", address);
    let app = Router::new()
        .route("/", get(index))
        .route("/{path}", get(redirect))
        .route("/{path}", post(upload))
        .route("/{lock}/{path}", get(download))
        .with_state(state)
        .layer(DefaultBodyLimit::max(1024*1024*1024*100));

    let listener = tokio::net::TcpListener::bind(address).await.expect("Could not listen to port");
    axum::serve(listener, app).await?;

    Ok(())
}

async fn index() -> &'static str { // this should be a landing page for the project to the github and such
    "If you were sent a link here, it probably doesn't exist anymore."
}

// this process is a little broken currently
async fn download(State(state): State<AppState>, Path((lock, path)): Path<(String, String)>) -> Result<impl IntoResponse, (StatusCode, Markup)> {
    debug!("Attempting download to token {lock} of file {path}");
    let readstate = state.downloads.lock().await;
    debug!("Lock acquired for {lock} (testing for file exist)");

    let file = match readstate.get(&(lock, path)) {
        Some(file) => file,
        None => return Err((StatusCode::NOT_FOUND, html! {"Not found"}))
    }.clone();
    let size = file.0;
    let data_hold = file.1.clone();
    drop(readstate);

    let s = stream! {
        debug!("Attempting to lock output stream");
        let mut recv = data_hold.lock().await;
        trace!("Locked output stream");
        loop {
            let data = recv.recv().await;
            match data {
                Some(data) => {
                    if data.is_empty() {
                        break;
                    }
                    yield Ok(data);
                },
                None => {
                    yield Err(format!("No more data"));
                    break;
                }
            }

        }
    };

    let body = Body::from_stream(s);
    
    if size != 0 {
        Ok(Response::builder()
        .header("content-length", size)
        .body(body)
        .unwrap())
    } else {
        Ok(body.into_response())
    }
}

async fn redirect(State(state): State<AppState>, Path(path): Path<String>, headers: HeaderMap, Query(params): Query<HashMap<String, String>>) -> Result<impl IntoResponse, (StatusCode, Markup)> {
    debug!("Attempting redirect download to {path}");
    let mut sysstate = state.files.lock().await;
    let file = match sysstate.get(&path) {
        Some(file) => file,
        None => return Err((StatusCode::NOT_FOUND, html! {"Not found"}))
    };
    debug!("File acquired for {path}");

    let user_agent = headers.get("User-Agent");

    let query_download: bool = match params.get("download") {
        Some(query_download) => match query_download.parse() {
            Ok(query_download) => query_download,
            Err(_) => false
        },
        None => false
    };
    let agent = match user_agent {
        Some(user_agent) => user_agent.to_str().unwrap(),
        None => ""
    };

    if (agent.starts_with("Mozilla") || agent.starts_with("WhatsApp")) && !query_download {
        debug!("User agent is web ({}), sending landing", agent);
        let file_size_string = format!("{} ({} bytes)", ByteSize(file.0 as u64).to_string_as(true), (&file.0));
        return Err((StatusCode::from_u16(200).unwrap(),
        html! { // this could be prettier, although it's not meant to be too complex
            (maud::DOCTYPE);
            html {
                head {
                    meta charset="utf-8";
                    meta name="viewport" content="width=device-width, initial-scale=1.0";
                    title {"Single-Use File Download: " (&path) }
                    meta property="og:title" content={"Single-Use File Download"};
                    meta property="og:description" content={"File download for " (&path) " [" (&file_size_string) "]"};
                }
                body {
                    h1 {"Single-Use File Download"}
                    p { "This download can only be started once. If it fails, you will need to ask the sender to re-upload"}
                    ul {
                        li {"File name: " (&path)}
                        li {"File size: " (&file_size_string)}
                    }
                    a href = "?download=true" download {"Click here to start the download"}
                    br;
                    i {"You may also download using curl or wget using this same url"} // should we give example commands?
                }
            }
        }
    ));
    }

    info!("Redirecting for {path}");

    // we need to generate the single-use page to redirect the user
    // this is not very secure, but it's a good start
    let mut hasher = Sha256::new();
    hasher.update(&path);
    let now = Utc::now();
    hasher.update(now.to_rfc2822());

    let download_path = format!("{:x}", hasher.finalize());

    // okay now we need to claim it!
    let elem = sysstate.remove(&path).expect("Path somehow doesnt exist?");
    //let mut recv = elem.1;

    // now we need to redirect to download
    let mut download_sys = state.downloads.lock().await;
    download_sys.insert((download_path.clone(), path.clone()), (elem.0, Arc::new(Mutex::new(elem.1))));

    return Ok(Redirect::temporary(&format!("/{}/{}", download_path, urlencoding::encode(&path)).to_string()));
}

#[axum::debug_handler]
async fn upload(State(state): State<AppState>, Path(path): Path<String>, mut multipart: Multipart) {
    let mut is_authed = false;
    let mut file_size = 0;
    while let Ok(field_raw) = multipart.next_field().await {
        let mut field = match field_raw {
            Some(field) => field,
            None => {
                error!("Form data incorrect, did the stream end early?");
                return;
            }
        };
        let name = field.name().unwrap().to_string();
        if name == "authentication" {
            debug!("User is attempting authentication");
            let content = field.text().await.unwrap();
            if content == state.token {
                is_authed = true;
                continue;
            } else {
                warn!("Authentication failed");
                return;
            }
        }
        if name == "file-size" && is_authed {
            debug!("User is attempting set size");
            let content = field.text().await.unwrap();
            file_size = content.parse::<usize>().unwrap();
            debug!("User set file size {}", file_size);
            continue;
        }
        if !is_authed {
            warn!("Authentication required for uploading, request cancelled");
            return;
        }
        let (tx, rx) = channel(state.cache_size);
        info!("Uploading new file {} to path {} (len: {})", name, path.clone(), file_size);
        let mut sysstate = state.files.lock().await;

        sysstate.remove(&path); // clean up in case another one already exists.. Perhaps we should error here
        sysstate.insert(path.clone(), (file_size, rx));
        drop(sysstate);
        
        let mut size = 0;
        info!("Upload to path {} had receiver... sending", name);
        while let Some(chunk) = field.chunk().await.unwrap() {
            size += chunk.len();
            //trace!("Sending chunk of size: {}", chunk.len());
            match tx.send(chunk.to_vec()).await {
                Ok(_) => (),
                Err(e) => {
                    error!("Failed to send chunk: {:?}", e);
                    return;
                }
            }
            if tx.is_closed() {
                error!("Upload failed");
                return;
            }
        }

        match tx.send(vec![]).await {
            Ok(_) => (),
            Err(e) => {
                error!("Failed to send final chunk: {:?}", e);
            }
        }
        info!("Sent file with size {} to path {}", size, &path);
        return;
    }
}