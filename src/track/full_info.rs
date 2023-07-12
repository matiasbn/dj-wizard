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
    pub info_url: String,
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
            info_url: "".to_string(),
            bpm: 0,
        }
    }

    pub async fn get_track_info(&mut self, soundeo_user: &SoundeoUser) -> SoundeoTrackResult<()> {
        let client = reqwest::Client::new();
        let info_url = format!("https://www.soundeo.com/tracks/status/{}", self.track_id);
        let session_cookie = soundeo_user
            .get_session_cookie()
            .change_context(SoundeoTrackError)?;
        let response = client
            .get(info_url.clone())
            .header("authority", "soundeo.com")
            .header("accept", "application/json, text/javascript, */*; q=0.01")
            .header("accept-language", "en-US,en;q=0.9")
            .header("content-type", "application/x-www-form-urlencoded; charset=UTF-8")
            .header("cookie", session_cookie)
            .header("sec-ch-ua", r#"Not.A/Brand";v="8", "Chromium";v="114", "Brave";v="114"#)
            .header("sec-ch-ua-mobile", "?0")
            .header("sec-ch-ua-platform", "macOS")
            .header("sec-fetch-dest", "empty")
            .header("sec-fetch-mode", "cors")
            .header("sec-fetch-site", "same-origin")
            .header("sec-gpc", "1")
            .header("user-agent", "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/114.0.0.0 Safari/537.36")
            .header("x-requested-with", "XMLHttpRequest")
            .send()
            .await.into_report().change_context(SoundeoTrackError)?;
        let response_text = response
            .text()
            .await
            .into_report()
            .change_context(SoundeoTrackError)?;
        let json_resp: Value = serde_json::from_str(&response_text)
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
        self.info_url = info_url;
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
