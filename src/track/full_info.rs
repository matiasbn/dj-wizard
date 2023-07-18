use crate::soundeo::api::SoundeoAPI;
use crate::track::{SoundeoTrackError, SoundeoTrackResult};
use crate::user::SoundeoUser;
use error_stack::{IntoReport, ResultExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Deserialize, Serialize, Debug, Default, Clone)]
pub enum SoundeoTrackGenre {
    #[default]
    Unknown,
    DrumAndBass,
}

impl SoundeoTrackGenre {
    pub fn from_string(genre_string: String) -> Self {
        return match genre_string.as_str() {
            "Drum & Bass" => Self::DrumAndBass,
            _ => Self::Unknown,
        };
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FullTrackInfo {
    pub track_id: String,
    pub format: String,
    pub title: String,
    pub size: String,
    pub genre: SoundeoTrackGenre,
    pub key: String,
    pub release: String,
    pub bpm: u64,
}

impl FullTrackInfo {
    pub fn new(track_id: String) -> Self {
        Self {
            track_id,
            format: "AIFF".to_string(),
            title: "".to_string(),
            size: "".to_string(),
            genre: Default::default(),
            key: "".to_string(),
            release: "".to_string(),
            bpm: 0,
        }
    }

    pub async fn get_track_info(&mut self, soundeo_user: &SoundeoUser) -> SoundeoTrackResult<()> {
        let api_response = SoundeoAPI::GetTrackInfo {
            track_id: self.track_id.clone(),
        }
        .call(soundeo_user)
        .await
        .change_context(SoundeoTrackError)?;
        let json_resp: Value = serde_json::from_str(&api_response)
            .into_report()
            .change_context(SoundeoTrackError)?;
        let track_resp = json_resp["track"]
            .clone()
            .as_object()
            .ok_or(SoundeoTrackError)
            .into_report()?
            .clone();
        self.title = track_resp["title"]
            .clone()
            .as_str()
            .ok_or(SoundeoTrackError)
            .into_report()?
            .to_string();
        self.bpm = track_resp["bpm"]
            .clone()
            .as_str()
            .ok_or(SoundeoTrackError)
            .into_report()?
            .to_string()
            .parse::<u64>()
            .into_report()
            .change_context(SoundeoTrackError)?;
        self.release = track_resp["release"]
            .clone()
            .as_str()
            .ok_or(SoundeoTrackError)
            .into_report()?
            .to_string();
        self.size = track_resp["format2size"]
            .clone()
            .as_str()
            .ok_or(SoundeoTrackError)
            .into_report()?
            .to_string();
        self.release = track_resp["release"]
            .clone()
            .as_str()
            .ok_or(SoundeoTrackError)
            .into_report()?
            .to_string();
        self.key = track_resp["key"]
            .clone()
            .as_str()
            .ok_or(SoundeoTrackError)
            .into_report()?
            .to_string();
        let genre_string = track_resp["genre"]
            .clone()
            .as_str()
            .ok_or(SoundeoTrackError)
            .into_report()?
            .to_string();
        self.genre = SoundeoTrackGenre::from_string(genre_string);
        Ok(())
    }
}
