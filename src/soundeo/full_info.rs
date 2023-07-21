use crate::soundeo::api::SoundeoAPI;
use crate::soundeo::{SoundeoError, SoundeoResult};
use crate::user::SoundeoUser;
use error_stack::{FutureExt, IntoReport, ResultExt};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

pub fn deserialize_to_number<'de, D>(deserializer: D) -> Result<u32, D::Error>
where
    D: Deserializer<'de>,
{
    let s: String = String::deserialize(deserializer)?;
    s.parse::<u32>().map_err(serde::de::Error::custom)
}

pub fn parse_soundeo_url<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let partial_url: String = String::deserialize(deserializer)?;
    Ok(format!("https://www.soundeo.com{}", partial_url))
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SoundeoTrackFullInfo {
    pub id: String,
    pub title: String,
    pub cover: String,
    #[serde(deserialize_with = "parse_soundeo_url")]
    pub track_url: String,
    pub release: String,
    pub label: String,
    pub genre: String,
    pub date: String,
    #[serde(deserialize_with = "deserialize_to_number")]
    pub bpm: u32,
    pub key: String,
    #[serde(rename(deserialize = "format2size"))]
    pub size: Option<String>,
    pub downloadable: bool,
}

impl SoundeoTrackFullInfo {
    pub fn new(id: String) -> Self {
        SoundeoTrackFullInfo {
            id,
            title: "".to_string(),
            cover: "".to_string(),
            track_url: "".to_string(),
            release: "".to_string(),
            label: "".to_string(),
            genre: "".to_string(),
            date: "".to_string(),
            bpm: 0,
            key: "".to_string(),
            size: Some("".to_string()),
            downloadable: false,
        }
    }
    pub async fn get_info(&mut self, soundeo_user: &SoundeoUser) -> SoundeoResult<()> {
        let api_response = SoundeoAPI::GetTrackInfo {
            track_id: self.id.clone(),
        }
        .get(soundeo_user)
        .await
        .change_context(SoundeoError)?;
        let json: Value = serde_json::from_str(&api_response)
            .into_report()
            .change_context(SoundeoError)?;
        let track = json["track"].clone();
        let full_info: Self = serde_json::from_value(track)
            .into_report()
            .change_context(SoundeoError)?;
        self.clone_from(&full_info);
        Ok(())
    }
    // fn parse_json_response(response: String) -> Sounde
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_get_info() {
        let track_id = "8068396".to_string();
        let mut soundeo_full_info = SoundeoTrackFullInfo::new(track_id);
        let mut soundeo_user = SoundeoUser::new().unwrap();
        soundeo_user.login_and_update_user_info().await.unwrap();
        soundeo_full_info.get_info(&soundeo_user).await.unwrap();
        println!("{:#?}", soundeo_full_info);
    }
}
