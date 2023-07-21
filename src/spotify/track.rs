use colored::Colorize;
use colorize::AnsiColor;
use error_stack::{Report, ResultExt};
use serde::{Deserialize, Serialize};

use crate::dialoguer::Dialoguer;
use crate::soundeo::search_bar::SoundeoSearchBar;
use crate::soundeo::track::SoundeoTrack;
use crate::spotify::{SpotifyError, SpotifyResult};
use crate::user::SoundeoUser;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SpotifyTrack {
    pub title: String,
    pub artists: String,
    pub spotify_track_id: String,
}

impl SpotifyTrack {
    pub fn new(title: String, artists: String, spotify_track_id: String) -> Self {
        Self {
            title,
            artists,
            spotify_track_id,
        }
    }

    pub async fn get_soundeo_track_id(
        &self,
        soundeo_user: &SoundeoUser,
    ) -> SpotifyResult<Option<String>> {
        let term = self.get_track_search_term();
        let results = SoundeoSearchBar::Tracks
            .search_term(term, &soundeo_user)
            .await
            .change_context(SpotifyError)?;
        if results.is_empty() {
            println!(
                "Tracks not found for song {} by {}: {}",
                self.title.clone().red(),
                self.artists.clone().red(),
                self.get_track_url()
            );
            return Ok(None);
        }
        let mut titles = vec![];
        for result in results.clone() {
            let id = result.value;
            let mut full_info = SoundeoTrack::new(id.clone());
            full_info
                .get_info(&soundeo_user)
                .await
                .change_context(SpotifyError)?;
            if full_info.downloadable {
                titles.push(format!("{} - {}", full_info.title, full_info.track_url));
            }
        }
        if titles.is_empty() {
            println!(
                "Track is not downloadable, {} by {}: {}",
                self.title.clone().red(),
                self.artists.clone().red(),
                self.get_track_url()
            );
            return Ok(None);
        }

        titles.push("Skip this track".purple().to_string());

        let prompt_text = format!(
            "Select the correct option for {} by {}: {}",
            self.title.clone().cyan(),
            self.artists.clone().cyan(),
            self.get_track_url()
        );

        let selection =
            Dialoguer::select(prompt_text, titles.clone(), None).change_context(SpotifyError)?;

        if selection == titles.len() - 1 {
            return Ok(None);
        }
        let search_result = results[selection].clone();
        Ok(Some(search_result.value))
    }

    pub fn get_track_search_term(&self) -> String {
        format!("{} - {}", self.artists, self.title)
    }

    pub fn get_track_url(&self) -> String {
        format!("https://open.spotify.com/track/{}", self.spotify_track_id)
    }
}
