use colored::Colorize;
use colorize::AnsiColor;
use error_stack::{IntoReport, Report, ResultExt};
use serde::{Deserialize, Serialize};

use crate::dialoguer::Dialoguer;
use crate::soundeo::search_bar::{SoundeoSearchBar, SoundeoSearchBarResult};
use crate::soundeo::track::SoundeoTrack;
use crate::spotify::{SpotifyError, SpotifyResult};
use crate::user::SoundeoUser;

// Enum to provide detailed results from the auto-pairing attempt.
pub enum AutoPairResult {
    Paired(String), // Contains the Soundeo track ID
    NoMatch,
    MultipleMatches,
}

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

    pub async fn find_single_soundeo_match(
        &mut self,
        soundeo_user: &SoundeoUser,
    ) -> SpotifyResult<AutoPairResult> {
        let downloadable_tracks = find_downloadable_soundeo_tracks(self, soundeo_user).await?;

        if downloadable_tracks.len() == 1 {
            // Exactly one match, perfect for auto-pairing.
            let (search_result, track_info) = &downloadable_tracks[0];
            Ok(AutoPairResult::Paired(search_result.value.clone()))
        } else if downloadable_tracks.is_empty() {
            Ok(AutoPairResult::NoMatch)
        } else {
            // Zero or more than one match, requires manual intervention.
            Ok(AutoPairResult::MultipleMatches)
        }
    }

    pub async fn get_soundeo_track_id(
        &self,
        soundeo_user: &SoundeoUser,
    ) -> SpotifyResult<Option<String>> {
        let downloadable_tracks = find_downloadable_soundeo_tracks(self, soundeo_user).await?;

        if downloadable_tracks.is_empty() {
            println!(
                "Tracks not found for song {} by {}: {}",
                self.title.clone().red(),
                self.artists.clone().red(),
                self.get_track_url()
            );
            return Ok(None); // No downloadable tracks found at all.
        }

        let mut titles: Vec<String> = downloadable_tracks
            .iter()
            .map(|(_, info)| format!("{} - {}", info.title, info.get_track_url()))
            .collect();

        if titles.len() == 1 {
            let track_data = format!(
                "{} by {}: {}",
                self.title.clone().cyan(),
                self.artists.clone().cyan(),
                self.get_track_url()
            );
            println!("Track found for {} \n {}", track_data, titles[0]);
            return Ok(Some(downloadable_tracks[0].0.value.clone()));
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
        let search_result = downloadable_tracks[selection].0.clone();
        Ok(Some(search_result.value))
    }

    pub fn get_track_search_term(&self) -> String {
        format!("{} - {}", self.artists, self.title)
    }

    pub fn get_track_url(&self) -> String {
        format!("https://open.spotify.com/track/{}", self.spotify_track_id)
    }
}

/// Helper function to search Soundeo and filter for downloadable tracks.
async fn find_downloadable_soundeo_tracks(
    spotify_track: &SpotifyTrack,
    soundeo_user: &SoundeoUser,
) -> SpotifyResult<Vec<(SoundeoSearchBarResult, SoundeoTrack)>> {
    let term = spotify_track.get_track_search_term();
    let results = SoundeoSearchBar::Tracks
        .search_term(term, soundeo_user)
        .await
        .change_context(SpotifyError)?;

    if results.is_empty() {
        return Ok(vec![]);
    }

    let mut downloadable_results = vec![];
    for result in results {
        let id = result.value.clone();
        let mut full_info = SoundeoTrack::new(id);
        full_info
            .get_info(soundeo_user, false)
            .await
            .change_context(SpotifyError)?;
        if full_info.downloadable {
            downloadable_results.push((result, full_info));
        }
    }

    Ok(downloadable_results)
}
