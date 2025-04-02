use std::{collections::HashMap, sync::{atomic::{AtomicUsize, Ordering}, Arc}};
use anyhow::Result;
use async_stream::stream;
use axum::{body::Body, extract::{DefaultBodyLimit, Multipart, Path, Query, State}, http::{HeaderMap, HeaderName, HeaderValue, Response, StatusCode}, response::{IntoResponse, Redirect}, routing::{delete, get, post}, Form, Json, Router};
use chrono::{Duration, TimeDelta};
use maud::{html, Markup};
use bytes::{BytesMut, BufMut};
use reqwest::header::{CONTENT_ENCODING, CONTENT_LENGTH};
use tracing::{debug, error, info, trace, warn};
use crate::{server::appstate::AppState, utils::{compression::Compression, metadata::FileMetadata}};
use tower_http::set_header::SetResponseHeaderLayer;
use std::str::FromStr;

use super::{serveropts::ServerOptions, ServerConfig};



pub async fn server(config: ServerConfig) -> Result<()> {
    let address = config.listen.expect("No server listen address defined");

    let public_config = match config.public_options {
        Some(public_options) => public_options,
        None => {
            warn!("Public config is not defined... Using defaults!");
            // limit of 4kbps to long UUID tokens
            ServerOptions::new(1, 4096, Duration::hours(1), "{uuid}".to_string(), "{uuid}".to_string(), Some(TimeDelta::seconds(1)), None)
        },
    };

    let authed_config = match config.authenticated_options {
        Some(authenticated_options) => authenticated_options,
        None => {
            warn!("Authenticated config is not defined... Using defaults!");
            ServerOptions::new((1024 * 1024 * 1024) / 4096, 4096, Duration::hours(1), "{number}-{word}-{word}-{word}".to_string(), "{number}-{word}-{word}-{word}".to_string(), None, None)
        },
    };

    let state = AppState::new(public_config, authed_config, config.keyserver, config.users).await;


    info!("Starting server listening on {}", address);
    let app = Router::new()
        .route("/", get(index))
        .route("/{token}", get(get_download)) // redirects to download of direct file name
        .route("/{token}", delete(remove_file))
        .route("/{token}/{path}", get(download)) // download using certain filename, gets confused with upload path though
        .route("/{token}", post(make_upload)) // generates a new upload for a certain filename
        .route("/{token}/{path}", post(upload)) // allows upload to a given token and key, only upload generator determines file name
        .with_state(state)
        .layer(DefaultBodyLimit::max(1024*1024*1024*100))
        .layer(SetResponseHeaderLayer::if_not_present(
            HeaderName::from_static("server"),
            HeaderValue::from_str(&format!("ByteBeam/{}", env!("CARGO_PKG_VERSION")))
                .unwrap(),
        ));

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

    // we need to see if this is actually an upload
    if meta.check_key(&path) {
        // you cannot download using the key name, this is supposed to be POSTed to, so this will act as the landing
        return Ok(html! { // some CSS would be nice
            (maud::DOCTYPE);
            html {
                head {
                    meta charset="utf-8";
                    meta name="viewport" content="width=device-width, initial-scale=1.0";
                    title {"ByteBeam File Upload" }
                    meta property="og:title" content={"ByteBeam Web Upload"};
                    meta property="og:description" content={"File Upload"};
                }
                body {
                    h1 {"ByteBeam File Upload"}
                    p { "You can only begin an upload once, if the upload fails you will need to ask for a new upload link"}
                    form method="POST" action=(format!("/{token}/{path}")) enctype="multipart/form-data" {
                        input name="file" type="file";
                        input type="submit" value="Upload";
                    }
                    p {"You can also upload the file using curl"}
                    tt {"curl -F 'file=@/path/to/file' http://this-url/and/path" }
                    // now we need to do the form. There should maybe be a JS progress bar or something...
                }
            }
            }.into_response());
    }

    if meta.download_locked() {
        if meta.download_finished() {
            return Err((StatusCode::GONE, html! {"File already downloaded"}));
        }
        return Err((StatusCode::CONFLICT, html! {"File being downloaded"}));
    }

    let mut download = match state.begin_download(&token).await {
        Some(dl) => dl,
        None => {
            error!("File is unlocked however the stream could not be obtained");
            return Err((StatusCode::INTERNAL_SERVER_ERROR, html! {"Internal Server Error"})) // this file should be freed!
        }
    };

    let bytes_counter = Arc::new(AtomicUsize::new(0));
    let bytes_counter_clone = bytes_counter.clone();

    // Spawn a separate tokio task to handle the updates
    let update_handle = {
        let state = state.clone();
        let token = token.clone();
        tokio::spawn(async move {
            let mut updown = (0, 0);
            
            loop {
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                
                let bytes = bytes_counter.swap(0, Ordering::Relaxed);
                if bytes > 0 {
                    updown = match state.increase_upload_download_numbers(&token, 0, bytes).await {
                        Some((uploaded, downloaded)) => (uploaded, downloaded),
                        None => {
                            warn!("Failed to get upload/download numbers");
                            updown
                        }
                    };
                }
            }
        })
    };

    let s = stream! {
        loop {
            let data = download.recv().await;
            match data {
                Some(data) => {
                    bytes_counter_clone.fetch_add(data.len(), Ordering::Relaxed);
                    if data.is_empty() {
                        debug!("No bytes remaining to read");
                        state.end(&token).await;
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
        // the download is complete
        let final_bytes = bytes_counter_clone.load(Ordering::Relaxed);
        state.increase_upload_download_numbers(&token, 0, final_bytes).await;
        state.end(&token).await;
        update_handle.abort();
        info!("Download complete for {}", token);
    };

    let body = Body::from_stream(s);

    let response = Response::new(body);
    let (mut parts, body) = response.into_parts();

    if let Some(content_length) = meta.file_size.get_content_length() {
        debug!("Writing content length as {}", content_length);
        parts.headers.insert(CONTENT_LENGTH, content_length.into());
    }

    if meta.get_compression() != Compression::None {
        debug!("Writing compression as {:?}", meta.get_compression());
        parts.headers.insert(CONTENT_ENCODING, HeaderValue::from_str(meta.get_compression().to_string().as_str()).unwrap());
    };

    Ok(Response::from_parts(parts, body))

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

    let return_metadata: bool = match params.get("status") {
        Some(m_str) => match m_str.parse() {
            Ok(q) => q,
            Err(_) => false
        },
        None => false
    };

    let stream_metadata: bool = match params.get("stream") {
        Some(m_str) => match m_str.parse() {
            Ok(q) => q,
            Err(_) => false
        },
        None => false
    };

    if stream_metadata {
        let s =  stream! {
            loop {
                let meta = match state.get_file_metadata(&token).await {
                    Some(meta) => meta,
                    None => {
                        debug!("Could not get streaming metadata! The file probably expired");
                        yield Err("");
                        break
                    }
                };

                match serde_json::to_string(&meta.redact()) {
                    Ok(s) => yield Ok(format!("{}\n", s)),
                    Err(_) => {
                        debug!("Could not format the redacted metadata to json!");
                        yield Err("");
                        break
                    }
                }
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            }
        };
        let body = Body::from_stream(s);
        return Ok(body.into_response());
    }


    if return_metadata {
        return Ok(Json(meta.redact()).into_response());
    }

    if meta.download_locked() {
        if meta.download_finished() {
            return Err((StatusCode::GONE, html! {"File already downloaded"}));
        }
        return Err((StatusCode::CONFLICT, html! {"File being downloaded"}));
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
        let file_size_string = meta.file_size.get_file_string();
        return Err((StatusCode::from_u16(200).unwrap(),
        html! { // this could be prettier, although it's not meant to be too complex
        // some simple CSS down the line may be helpful
            (maud::DOCTYPE);
            html {
                head {
                    meta charset="utf-8";
                    meta name="viewport" content="width=device-width, initial-scale=1.0";
                    title {"ByteBeam File Download: " (&meta.file_name) }
                    meta property="og:title" content={"ByteBeam File Download"};
                    meta property="og:description" content={"File download for " (&meta.file_name) " [" (&file_size_string) "]"};
                }
                body {
                    h1 {"ByteBeam File Download"}
                    p { "This download can only be started once. If it fails, you will need to ask the sender to re-upload"}
                    ul {
                        li {"File name: " (&meta.file_name)}
                        li {"Uncompressed file size: " (&file_size_string)}
                        li {"Compression: " (&meta.get_compression().to_string())}
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
    Ok(Redirect::temporary(format!("/{token}/{}", meta.file_name).as_str()).into_response())

}

// this will return a lock/link to do the upload to
#[axum::debug_handler]
async fn make_upload(State(state): State<AppState>, Path(path): Path<String>, Form(params): Form<HashMap<String, String>>) -> Result<Json<FileMetadata>, (StatusCode, Markup)> {
    // new: anyone can call for an upload token, however it will be limited unless authenticated
    // rate limits may be good to add here, collisions are highly unlikely with uuids, however dealing with this takes compute!

    // this effectively has two paths, of "path" is a token, this is an upgrade 
    match state.get_file_metadata(&path).await {
        Some(_) => { // we have to do an upgrade
            let challenge = match params.get("challenge") {
                Some(challenge) => challenge,
                None => return Err((StatusCode::BAD_REQUEST, html! {"Missing challenge parameter"})),
            };

            // allows JSON but also will allow single entry
            let tests: Vec<String> = match serde_json::from_str(&challenge) {
                Ok(tests) => tests,
                Err(_) => vec![challenge.to_string()],
            };

            let resp = match state.upgrade(&path, &tests).await {
                Some(metadata) => {
                    debug!("Challenge passed. New metadata: {:?}", metadata);
                    metadata
                },
                None => return Err((StatusCode::UNAUTHORIZED, html! {"Challenge failed"})),
            };

            Ok(Json(resp))
        },
        None => { // we are doing a new upload
            let username = params.get("user");
            debug!("{:?}", username);
            match state.generate_file_upload(&path, username).await {
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
        }
    }
}

async fn upload(State(state): State<AppState>, Path((token, key)): Path<(String, String)>, mut multipart: Multipart) -> impl IntoResponse { // "path" is actually the key
    
    let (upload, upload_options) = match state.begin_upload(&token, &key).await {
        Ok(res) => res,
        Err(e) => {
            return e.into_response();
        }
    };

    let block_size = upload_options.get_block_size();
    let delay_time = upload_options.get_delay_time();

    trace!("Starting upload for {} with a delay size of {:?}", token, delay_time);

    // now we just need to allow the upload!
    while let Ok(field_raw) = multipart.next_field().await {
        let mut field = match field_raw {
            Some(field) => field,
            None => {
                error!("Form data incorrect, did the stream end early?");
                return "Form data incorrect, did the stream end early?".into_response();
            }
        };
        let name = field.name().unwrap().to_string();
        
        // TODO: small chance this can be done with hinting
        if name == "file-size" {
            debug!("User is attempting set size");
            let content = field.text().await.unwrap();
            // DONT unwrap the parse here!
            state.set_metadata(&token, None, Some(content.parse::<usize>().unwrap()), None).await;
            debug!("User set file size {}", content);
            continue;
        }

        if name == "compression" {
            debug!("User is attempting set compression");
            let content = field.text().await.unwrap();
            // DONT unwrap the parse here!
            // does it matter?
            state.set_metadata(&token, None, None, Some(Compression::from_str(content.as_str()).unwrap())).await;
            debug!("User set compression {}", content);
            continue;
        }

        // now get upload things
        info!("Upload to path {} had receiver... sending", name);

        let mut buffer = BytesMut::new();
        let bytes_counter = Arc::new(AtomicUsize::new(0));
        let bytes_counter_clone = bytes_counter.clone();

        // Spawn a separate tokio task to handle the updates
            let update_handle = {
            let state = state.clone();
            let token = token.clone();
            tokio::spawn(async move {
                let mut updown = (0, 0);
                
                loop {
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                    
                    let bytes = bytes_counter.swap(0, Ordering::Relaxed);
                    if bytes > 0 {
                        updown = match state.increase_upload_download_numbers(&token, bytes, 0).await {
                            Some((uploaded, downloaded)) => (uploaded, downloaded),
                            None => {
                                warn!("Failed to get upload/download numbers");
                                updown
                            }
                        };
                    }
                }
            })
        };

        while let Some(chunk) = field.chunk().await.unwrap() {
            bytes_counter_clone.fetch_add(chunk.len(), Ordering::Relaxed);
            buffer.put(chunk);

            while buffer.len() >= block_size {
                let chunk_data = buffer.split_to(block_size).to_vec();
                match upload.send(chunk_data).await {
                    Ok(_) => (),
                    Err(e) => {
                        error!("Failed to send chunk: {:?}. Upload ended prematurely?", e);
                        return "Failed to send a chunk... upload may have failed".into_response();
                    }
                }


                if upload.is_closed() {
                    error!("Upload failed");
                    return "Upload failed".into_response();
                }
                // we dont need to delay or try to if it doesnt exist
                if let Some(delay) = delay_time {
                    let std_duration = std::time::Duration::from_millis(delay.num_milliseconds() as u64); // micro/nano may be a better idea
                    tokio::time::sleep(std_duration).await;
                }
            }
        }

        match upload.send(buffer.to_vec()).await {
            Ok(_) => (),
            Err(e) => {
                error!("Failed to send final chunk: {:?}", e);
            }
        }

        match upload.send(vec![]).await {
            Ok(_) => (),
            Err(e) => {
                error!("Failed to send close signal: {:?}", e);
            }
        }

        let final_bytes = bytes_counter_clone.load(Ordering::Relaxed);
        state.increase_upload_download_numbers(&token, 0, final_bytes).await;
        state.end(&token).await;
        update_handle.abort();

        info!("Sent file with size {} to token {}", final_bytes, &token);
        // now we can mark upload as complete
        if state.end_upload(&token).await {
            return format!("Done! Sent {} bytes", final_bytes).into_response();
        } else { // this shouldn't really happen?
            error!("Had an issue marking the download as ended");
            return format!("Done! Sent {} bytes, however the upload failed to be marked as complete", final_bytes).into_response();
        }
    }
    return format!("An error occured (form has incomplete fields)").into_response();
}

async fn remove_file(State(state): State<AppState>, Path(token): Path<String>) { // "path" is actually the key
    state.delete(&token).await;
}