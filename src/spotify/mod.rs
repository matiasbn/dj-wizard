use std::collections::HashMap;
use std::fmt;

use error_stack::IntoReport;
use serde::{Deserialize, Serialize};

use crate::dialoguer::Dialoguer;
use crate::log::DjWizardLog;
use crate::log::DjWizardLogResult;
use crate::soundeo::search_bar::SoundeoSearchBarResult;
use crate::spotify::playlist::SpotifyPlaylist;

pub mod api;
pub mod commands;
pub mod playlist;
pub mod track;

#[derive(Debug)]
pub struct SpotifyError;

impl fmt::Display for SpotifyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Spotify error")
    }
}

impl std::error::Error for SpotifyError {}

pub type SpotifyResult<T> = error_stack::Result<T, SpotifyError>;

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Spotify {
    pub playlists: HashMap<String, SpotifyPlaylist>,
    pub soundeo_track_ids: HashMap<String, Option<String>>,
    #[serde(default)]
    pub multiple_matches_cache: HashMap<String, Vec<SoundeoSearchBarResult>>,
}

impl Spotify {
    pub fn new() -> Self {
        Self {
            playlists: HashMap::new(),
            soundeo_track_ids: HashMap::new(),
            multiple_matches_cache: HashMap::new(),
        }
    }

    pub fn get_playlist_by_name(&self, name: String) -> SpotifyResult<SpotifyPlaylist> {
        let playlist = self
            .playlists
            .values()
            .find(|playlist| playlist.name == name)
            .ok_or(SpotifyError)
            .into_report()?;
        Ok(playlist.clone())
    }
}

pub trait SpotifyCRUD {
    fn update_spotify_playlist(spotify_playlist: SpotifyPlaylist) -> DjWizardLogResult<()>;

    fn update_spotify_to_soundeo_track(
        spotify_track_id: String,
        soundeo_track_id: Option<String>,
    ) -> DjWizardLogResult<()>;

    fn delete_spotify_playlists(playlist_ids: &[String]) -> DjWizardLogResult<()>;

    fn add_to_multiple_matches_cache(
        spotify_id: String,
        results: Vec<SoundeoSearchBarResult>,
    ) -> DjWizardLogResult<()>;
}
