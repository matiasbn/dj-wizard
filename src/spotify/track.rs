use crate::dialoguer::Dialoguer;
use crate::soundeo::full_info::SoundeoTrackFullInfo;
use crate::soundeo::search_bar::SoundeoSearchBar;
use crate::spotify::{SpotifyError, SpotifyResult};
use crate::user::SoundeoUser;
use error_stack::{Report, ResultExt};
use serde::{Deserialize, Serialize};

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

    pub async fn get_soundeo_track_id(&self, soundeo_user: &SoundeoUser) -> SpotifyResult<String> {
        let term = self.get_track_search_term();
        let results = SoundeoSearchBar::Tracks
            .search_term(term, &soundeo_user)
            .await
            .change_context(SpotifyError)?;
        let mut titles = vec![];
        for result in results.clone() {
            let id = result.value;
            let mut full_info = SoundeoTrackFullInfo::new(id.clone());
            full_info
                .get_info(&soundeo_user)
                .await
                .change_context(SpotifyError)?;
            if full_info.downloadable {
                titles.push(format!("{} - {}", full_info.title, full_info.track_url));
            }
        }
        println!("{:#?}", titles);
        let prompt_text = "Select the correct option".to_string();
        let selection =
            Dialoguer::select(prompt_text, titles, None).change_context(SpotifyError)?;
        let search_result = results[selection].clone();
        Ok(search_result.value)
    }

    pub fn get_track_search_term(&self) -> String {
        format!("{} - {}", self.artists, self.title)
    }
}
