use std::{fmt, str::FromStr};
use serde::{Deserialize, Serialize};

// Reqwest supports various forms of compression, however doing it ourselves allows for more types,
// and allows for more control over the compression process

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
pub enum Compression {
    None,
    Brotli,
    Deflate, // flate2
    Gzip, // flate2
    Zstd,
}

impl fmt::Display for Compression {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Compression::None => write!(f, "none"),
            Compression::Gzip => write!(f, "gzip"),
            Compression::Deflate => write!(f, "deflate"),
            Compression::Brotli => write!(f, "br"),
            Compression::Zstd => write!(f, "zstd"),
        }
    }
}

impl FromStr for Compression {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "none" => Ok(Compression::None),
            "gzip" => Ok(Compression::Gzip),
            "deflate" => Ok(Compression::Deflate),
            "br" => Ok(Compression::Brotli),
            "zstd" => Ok(Compression::Zstd),
            _ => Err(format!("Unknown compression type: {}", s)),
        }
    }
}

impl Default for Compression {
    fn default() -> Self {
        Compression::None
    }
}