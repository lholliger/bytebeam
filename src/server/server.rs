use std::collections::HashMap;
use anyhow::Result;
use async_stream::stream;
use axum::{body::Body, extract::{DefaultBodyLimit, Multipart, Path, Query, State}, http::{HeaderMap, Response, StatusCode}, response::{IntoResponse, Redirect}, routing::{get, post}, Form, Json, Router};
use bytesize::ByteSize;
use maud::{html, Markup};
use tracing::{debug, error, info, warn};
use crate::{server::appstate::AppState, utils::metadata::FileMetadata};

pub async fn server(address: String, data_storage: usize, token: String) -> Result<()> {
    const CHUNK_SIZE: usize = 4096; // this is being assumed, it shouldn't be
    let cache_size: usize = data_storage / CHUNK_SIZE;

    debug!("Cache size bytes: {}", cache_size);

    if token == "password" {
        warn!("WARNING - Using the default password is not recommended. Please use a secure token.")
    }

    let state = AppState::new(&token, cache_size);

    info!("Starting server listening on {}", address);
    let app = Router::new()
        .route("/", get(index))
        .route("/{token}", get(get_download)) // redirects to download of direct file name
        .route("/{token}/{path}", get(download)) // download using certain filename
        .route("/{token}", post(make_upload)) // generates a new upload for a certain filename
        .route("/{token}/{path}", post(upload)) // allows upload to a given token and key, only upload generator determines file name
        .with_state(state)
        .layer(DefaultBodyLimit::max(1024*1024*1024*100));

    let listener = tokio::net::TcpListener::bind(address).await.expect("Could not listen to port");
    axum::serve(listener, app).await?;

    Ok(())
}

async fn index() -> &'static str { // this should be a landing page for the project to the github and such
    "If you were sent a link here, it probably doesn't exist anymore."
}

async fn download(State(state): State<AppState>, Path((token, path)): Path<(String, String)>) -> Result<impl IntoResponse, (StatusCode, Markup)> {
    // we could check the path, but its quite honestly not needed and the user should be able to do what they want
    debug!("Attempting download to {token}/{path}");
    let meta = match state.get_file_metadata(&token).await {
        Some(meta) => meta,
        None => {
            return Err((StatusCode::NOT_FOUND, html! {"File not found"}));
        }
    };

    if meta.download_locked() {
        warn!("File already being downloaded, sending error");
        return Err((StatusCode::CONFLICT, html! {"File already being downloaded"}));
    }

    let mut download = match state.begin_download(&token).await {
        Some(dl) => dl,
        None => {
            error!("File is unlocked however the stream could not be obtained");
            return Err((StatusCode::INTERNAL_SERVER_ERROR, html! {"Internal Server Error"})) // this file should be freed!
        }
    };

    let s = stream! {
        loop {
            let data = download.recv().await;
            match data {
                Some(data) => {
                    if data.is_empty() {
                        break;
                    }
                    yield Ok(data);
                },
                None => {
                    yield Err(format!("Download possibly dropped?"));
                    break;
                }
            }
        }
        warn!("Download seems to have ended prematurely"); // this line is never really reached
    };

    let body = Body::from_stream(s);
    
    if meta.file_size != 0 {
        Ok(Response::builder()
        .header("content-length", meta.file_size)
        .body(body)
        .unwrap())
    } else {
        Ok(body.into_response())
    }

    // on fail, return the downloader
}

async fn get_download(State(state): State<AppState>, Path(token): Path<String>, headers: HeaderMap, Query(params): Query<HashMap<String, String>>) -> Result<impl IntoResponse, (StatusCode, Markup)> {
    debug!("Attempting download check to {token}");
    let meta = match state.get_file_metadata(&token).await {
        Some(meta) => meta,
        None => {
            return Err((StatusCode::NOT_FOUND, html! {"File not found"}));
        }
    };

    if meta.download_locked() {
        return Err((StatusCode::CONFLICT, html! {"File already being downloaded"}));
    }

    debug!("File is allowed for download {token}");

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
        let file_size_string = format!("{} ({} bytes)", ByteSize(meta.file_size as u64).to_string_as(true), (meta.file_size));
        return Err((StatusCode::from_u16(200).unwrap(),
        html! { // this could be prettier, although it's not meant to be too complex
        // some simple CSS down the line may be helpful
            (maud::DOCTYPE);
            html {
                head {
                    meta charset="utf-8";
                    meta name="viewport" content="width=device-width, initial-scale=1.0";
                    title {"Single-Use File Download: " (&meta.file_name) }
                    meta property="og:title" content={"Single-Use File Download"};
                    meta property="og:description" content={"File download for " (&meta.file_name) " [" (&file_size_string) "]"};
                }
                body {
                    h1 {"Single-Use File Download"}
                    p { "This download can only be started once. If it fails, you will need to ask the sender to re-upload"}
                    ul {
                        li {"File name: " (&meta.file_name)}
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

    // nothing is locked so we can just redirect

    debug!("Redirecting download to {token}/{}", meta.file_name);
    Ok(Redirect::temporary(format!("/{token}/{}", meta.file_name).as_str()))

}

// this will return a lock/link to do the upload to
#[axum::debug_handler]
async fn make_upload(State(state): State<AppState>, Path(path): Path<String>, Form(params): Form<HashMap<String, String>>,) -> Result<Json<FileMetadata>, (StatusCode, Markup)> {
    if let Some(auth) = params.get("authentication") {
        debug!("Attempting to generate lock token for {path}");

        match state.generate_file_upload(auth, &path).await {
            Some(file_metadata) => {
                debug!("Generated upload token for {path}");
                // we may also want to allow options to be included in the upload
                Ok(Json(file_metadata))
            },
            None => {
                debug!("Failed to generate lock token for {path}. User likely did not use main token");
                Err((StatusCode::UNAUTHORIZED, html! {"Unauthorized" }))
            }
        }
    } else {
        return Err((StatusCode::UNAUTHORIZED, html! {"Unauthorized. Authentication not provided" }));
    }
}

// TODO: give useful output when something fails?
#[axum::debug_handler]
async fn upload(State(state): State<AppState>, Path((token, key)): Path<(String, String)>, mut multipart: Multipart) { // "path" is actually the key
    let upload = match state.begin_upload(&token, &key).await { // wont keep the metadata locked for too long, hopefully!
        Some(sender) => {
            sender
        }
        None => {
            error!("Upload does not exist, or the password was incorrect, cannot proceed with upload");
            return;
        }
    };

    // now we just need to allow the upload!
    while let Ok(field_raw) = multipart.next_field().await {
        let mut field = match field_raw {
            Some(field) => field,
            None => {
                error!("Form data incorrect, did the stream end early?");
                return;
            }
        };
        let name = field.name().unwrap().to_string();
        
        // TODO: small chance this can be done with hinting
        if name == "file-size" {
            debug!("User is attempting set size");
            let content = field.text().await.unwrap();
            // DONT unwrap the parse here!
            state.set_metadata(&token, None, Some(content.parse::<usize>().unwrap())).await;
            debug!("User set file size {}", content);
            continue;
        }

        // now get upload things
        let mut size = 0;
        info!("Upload to path {} had receiver... sending", name);
        while let Some(chunk) = field.chunk().await.unwrap() {
            size += chunk.len();
            //trace!("Sending chunk of size: {}", chunk.len());
            match upload.send(chunk.to_vec()).await {
                Ok(_) => (),
                Err(e) => {
                    error!("Failed to send chunk: {:?}. Upload ended prematurely?", e);
                    return;
                }
            }
            if upload.is_closed() {
                error!("Upload failed");
                return;
            }
        }

        match upload.send(vec![]).await {
            Ok(_) => (),
            Err(e) => {
                error!("Failed to send final chunk: {:?}", e);
            }
        }
        info!("Sent file with size {} to token {}", size, &token);
        return;
    }
}