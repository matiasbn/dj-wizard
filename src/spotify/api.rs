use std::fmt;
use std::fmt::Write;

use error_stack::{IntoReport, ResultExt};

use crate::spotify::Spotify;

#[derive(Debug)]
pub struct SpotifyAPIError;
impl fmt::Display for SpotifyAPIError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SpotifyAPI error")
    }
}
impl std::error::Error for SpotifyAPIError {}

pub type SpotifyAPIResult<T> = error_stack::Result<T, SpotifyAPIError>;

pub enum SpotifyAPI {
    GetPlaylist { playlist_url: String },
}

impl SpotifyAPI {
    pub async fn get(&self) -> SpotifyAPIResult<String> {
        return match self {
            SpotifyAPI::GetPlaylist { playlist_url } => {
                let response = self.api_get(playlist_url.clone()).await?;
                Ok(response)
            }
        };
    }

    async fn api_get(&self, url: String) -> SpotifyAPIResult<String> {
        let client = reqwest::Client::new();
        let response = client
            .get(url)
            .send()
            .await
            .into_report()
            .change_context(SpotifyAPIError)?;
        let response_text = response
            .text()
            .await
            .into_report()
            .change_context(SpotifyAPIError)?;
        Ok(response_text)
    }
}
