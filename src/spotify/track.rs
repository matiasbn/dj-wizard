use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SpotifyTrack {
    pub title: String,
    pub artists: String,
}

impl SpotifyTrack {
    pub fn new(title: String, artists: String) -> Self {
        Self { title, artists }
    }
}
