use std::collections::HashMap;
use std::fmt;

use crate::spotify::playlist::SpotifyPlaylist;

pub mod api;
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

pub struct Spotify {
    playlists: HashMap<String, SpotifyPlaylist>,
}
