mod assets;
mod upload;

pub use assets::{AssetIndex, AssetMetadata, KVAssets};

// for non-wasm, export asset builders that depend on std::fs and wrangler libs
#[cfg(not(target_arch = "wasm32"))]
pub use upload::{sync_assets, SyncConfig};

use thiserror::Error as ThisError;
#[derive(Debug, ThisError)]
pub enum Error {
    #[error("KV Api error {0}")]
    KVHttp(reqwest::Error),

    #[error("KV Key {0} not found. status={1}")]
    KVKeyNotFound(String, u16),

    #[error("Deserializing assets:{0}")]
    DeserializeAssets(bincode::Error),

    #[error("Empty key passed to lookup")]
    EmptyKey,

    #[cfg(not(target_arch = "wasm32"))]
    #[error("Wangler error: {0}")]
    Wrangler(String),

    #[cfg(not(target_arch = "wasm32"))]
    #[error("IO Error: {0}")]
    IO(String),

    #[cfg(not(target_arch = "wasm32"))]
    #[error("Invalid asset output path: {0}")]
    InvalidAssetsBinPath(String),

    #[cfg(not(target_arch = "wasm32"))]
    #[error("creating output directory {0}")]
    CreateDir(String),

    #[cfg(not(target_arch = "wasm32"))]
    #[error("Invalid asset path: not a directory: {0}")]
    InvalidAssetPath(String),

    #[cfg(not(target_arch = "wasm32"))]
    #[error("Missing config file {0}")]
    MissingWranglerFile(String),

    #[error("TTL to short. Must be at least 60 seconds")]
    TTLTooShort,

    // catch-all
    #[error("{0}")]
    Message(String),
}

#[cfg(not(target_arch = "wasm32"))]
impl From<failure::Error> for Error {
    fn from(e: failure::Error) -> Error {
        Error::Wrangler(format!("{:?}", e))
    }
}
