use async_stream::stream;
use bytes::Bytes;
use flate2::write::{GzEncoder, DeflateEncoder};
use tokio_stream::Stream;
use std::sync::{Arc, Mutex};
use std::io::Write;
use tokio_stream::StreamExt;
use tracing::trace;

use crate::utils::compression::Compression;

pub struct ProgressStream<S> {
    reader_stream: S,
    int_read: Arc<Mutex<u64>>,
    progress_bar: indicatif::ProgressBar,
    compression: Compression,
}

impl<S> ProgressStream<S> where S: Stream<Item = Result<Bytes, std::io::Error>> + Unpin, {
    pub fn new(
        reader_stream: S, 
        int_read: Arc<Mutex<u64>>, 
        progress_bar: indicatif::ProgressBar,
        compression: Compression,
    ) -> Self {
        Self {
            reader_stream,
            int_read,
            progress_bar,
            compression,
        }
    }

    pub fn into_stream(self) -> impl Stream<Item = Result<Bytes, std::io::Error>> {
        let Self { 
            mut reader_stream, 
            int_read, 
            progress_bar: bar,
            compression,
        } = self;

        stream! {
            match compression {
                Compression::None => {
                    while let Some(chunk) = reader_stream.next().await {
                        if let Ok(chunk) = &chunk {
                            let mut b = int_read.lock().unwrap();
                            *b += chunk.len() as u64;
                            bar.set_position(*b);
                        }
                        yield chunk;
                    }
                },
                Compression::Gzip => {
                    let mut encoder = GzEncoder::new(Vec::new(), flate2::Compression::default());
                    while let Some(chunk) = reader_stream.next().await {
                        if let Ok(chunk) = &chunk {
                            {
                                let mut b = int_read.lock().unwrap();
                                *b += chunk.len() as u64;
                                bar.set_position(*b);
                            }
                            
                            if let Ok(_) = encoder.write_all(&chunk) {
                                // Get a mutable reference to the underlying Vec<u8>
                                let compressed_data = encoder.get_mut();
                                let compressed_chunk = compressed_data.clone();
                                compressed_data.clear();
                                yield Ok(Bytes::from(compressed_chunk));
                            }
                        } else {
                            yield chunk;
                        }
                    }
                    if let Ok(remaining) = encoder.finish() {
                        if !remaining.is_empty() {
                            yield Ok(remaining.into());
                        }
                    }
                },
                Compression::Deflate => {
                    let mut encoder = DeflateEncoder::new(Vec::new(), flate2::Compression::default());
                    while let Some(chunk) = reader_stream.next().await {
                        if let Ok(chunk) = &chunk {
                            {
                                let mut b = int_read.lock().unwrap();
                                *b += chunk.len() as u64;
                                bar.set_position(*b);
                            }
                            
                            if let Ok(_) = encoder.write_all(&chunk) {
                                let compressed_data = encoder.get_mut();
                                let compressed_chunk = compressed_data.clone();
                                compressed_data.clear();
                                yield Ok(Bytes::from(compressed_chunk));
                            }
                        } else {
                            yield chunk;
                        }
                    }
                    if let Ok(remaining) = encoder.finish() {
                        if !remaining.is_empty() {
                            yield Ok(remaining.into());
                        }
                    }
                },
                Compression::Brotli => {
                    let mut encoder = brotli::CompressorWriter::new(Vec::new(), 1024*16, 7, 0);
                    while let Some(chunk) = reader_stream.next().await {
                        if let Ok(chunk) = &chunk {
                            {
                                let mut b = int_read.lock().unwrap();
                                *b += chunk.len() as u64;
                                bar.set_position(*b);
                            }
                            
                            if let Ok(_) = encoder.write_all(&chunk) {
                                let compressed_data = encoder.get_mut();
                                let compressed_chunk = compressed_data.clone();
                                compressed_data.clear();
                                yield Ok(Bytes::from(compressed_chunk));
                            }
                        } else {
                            yield chunk;
                        }
                    }
                    // clean up
                    if let Ok(_) = encoder.flush() {
                        let final_encoder = encoder.into_inner();
                        if !final_encoder.is_empty() {
                            yield Ok(Bytes::from(final_encoder));
                        }
                    }
                },
                Compression::Zstd => {
                    let mut encoder = zstd::stream::Encoder::new(Vec::new(), 3).unwrap();
                    while let Some(chunk) = reader_stream.next().await {
                        if let Ok(chunk) = &chunk {
                            {
                                let mut b = int_read.lock().unwrap();
                                *b += chunk.len() as u64;
                                bar.set_position(*b);
                            }
                            
                            if let Ok(_) = encoder.write_all(&chunk) {
                                let compressed_data = encoder.get_mut();
                                let compressed_chunk = compressed_data.clone();
                                compressed_data.clear();
                                yield Ok(Bytes::from(compressed_chunk));
                            }
                        } else {
                            trace!("Done?");
                            yield chunk;
                        }
                    }
                    if let Ok(final_buffer) = encoder.finish() {
                        if !final_buffer.is_empty() {
                            yield Ok(Bytes::from(final_buffer));
                        }
                    }
                }
            }
        }
    }
}