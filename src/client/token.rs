use tracing::{debug, error, warn};

use crate::utils::metadata::FileMetadata;

pub async fn get_upload_token(auth: String, file_len: usize, request_path: String) -> Option<FileMetadata> {
    let params = [("authentication", auth), ("file-size", file_len.to_string())];

    let client = reqwest::Client::new();
    let res = client.post(request_path)
        .form(&params)
        .send().await;

    debug!("Request: {:?}", res);

    let metadata: FileMetadata = match res {
        Ok(response) => {
            if !response.status().is_success() {
                error!(
                    "Non-success response from Beam server: {:?}", response.text().await
                );
                return None;
            }
            let wanted_version = format!("ByteBeam/{}", env!("CARGO_PKG_VERSION"));
            // warn if the versions are different
            match response.headers().get("server") {
                Some(version) => match version.to_str() {
                    Ok(version_str) => if version_str != wanted_version {
                        warn!("ByteBeam Server version does not match the expected version. It may be outdated and there may be instability! Got {}, wanted {}", version_str, wanted_version);
                    }
                    Err(_) => warn!("ByteBeam Server did not return a proper version string. It may be outdated and there may be instability!")
                }
                None => {
                    warn!("ByteBeam Server did not return a version. It may be outdated and there may be instability!");
                }
            }
            match response.json::<FileMetadata>().await {
                Ok(metadata) => metadata,
                Err(e) => {
                    error!("Failed to parse file metadata: {:?}.", e);
                    return None;
                }
            }
        },
        Err(e) => {
            error!("Failed to connect to Beam server: {:?}", e);
            return None;
        }
    };


    debug!("File metadata received: {:?}", metadata);

    Some(metadata)
}