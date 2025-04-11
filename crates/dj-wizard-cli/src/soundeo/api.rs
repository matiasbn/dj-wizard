use std::fmt;
use std::fmt::Write;

use error_stack::{IntoReport, ResultExt};

use crate::user::SoundeoUser;

#[derive(Debug)]
pub struct SoundeoAPIError;
impl fmt::Display for SoundeoAPIError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SoundeoAPI error")
    }
}
impl std::error::Error for SoundeoAPIError {}

pub type SoundeoAPIResult<T> = error_stack::Result<T, SoundeoAPIError>;

pub enum SoundeoAPI {
    GetTrackInfo { track_id: String },
    GetTrackDownloadUrl { track_id: String },
    GetSearchBarResult { term: String },
}

impl SoundeoAPI {
    pub async fn get(&self, soundeo_user: &SoundeoUser) -> SoundeoAPIResult<String> {
        return match self {
            SoundeoAPI::GetTrackInfo { track_id } => {
                let url = format!("https://soundeo.com/tracks/status/{}", track_id);
                let response = self.api_get(url, soundeo_user).await?;
                Ok(response)
            }
            SoundeoAPI::GetTrackDownloadUrl { track_id } => {
                let url = format!("https://soundeo.com/download/{}/3", track_id);
                let response = self.api_get(url, soundeo_user).await?;
                Ok(response)
            }
            SoundeoAPI::GetSearchBarResult { term } => {
                let url = format!("https://soundeo.com/catalog/ajAutocomplete?term={}", term);
                let response = self.api_get(url, soundeo_user).await?;
                Ok(response)
            }
        };
    }

    async fn api_get(&self, url: String, soundeo_user: &SoundeoUser) -> SoundeoAPIResult<String> {
        let client = reqwest::Client::new();
        let session_cookie = soundeo_user
            .get_session_cookie()
            .change_context(SoundeoAPIError)?;
        let response = client
            .get(url)
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
            .await.into_report().change_context(SoundeoAPIError)?;
        let response_text = response
            .text()
            .await
            .into_report()
            .change_context(SoundeoAPIError)?;
        Ok(response_text)
    }
}
