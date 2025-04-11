// crates/dj-wizard-core/src/error.rs
use thiserror::Error;

#[derive(Error, Debug)]
pub enum CoreError {
    #[error("Configuration Error: {0}")]
    Config(String),

    #[error("I/O Error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Network Error: {0}")]
    Network(#[from] reqwest::Error),

    #[error("JSON Parsing Error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Soundeo Interaction Error: {0}")]
    Soundeo(String),

    #[error("Spotify Interaction Error: {0}")]
    Spotify(String),

    #[error("Login Failed: {0}")]
    Login(String),

    #[error("Scraping Error: {0}")]
    Scraping(String),

    #[error("Headless Browser Error: {0}")]
    Browser(String),

    #[error("Resource Not Found: {0}")]
    NotFound(String),

    #[error("Duplicate Cleaner Error: {0}")]
    Cleaner(String),

    #[error("Log State Error: {0}")]
    Log(String),

    #[error("Invalid Input: {0}")]
    Input(String),

    #[error("IPFS Error: {0}")]
    Ipfs(String),

    #[error("Unknown Error: {0}")]
    Unknown(String),

    #[error("URL Parsing Error: {0}")]
    UrlParse(#[from] url::ParseError),

    #[error("Environment Variable Error: {0}")]
    VarError(#[from] std::env::VarError),
}

pub type Result<T> = std::result::Result<T, CoreError>;
