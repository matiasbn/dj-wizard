use crate::soundeo::api::SoundeoAPI;
use crate::soundeo::{SoundeoError, SoundeoResult};
use crate::soundeo_log::DjWizardLog;
use crate::user::SoundeoUser;
use colorize::AnsiColor;
use error_stack::{FutureExt, IntoReport, ResultExt};
use futures_util::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::cmp::min;
use std::fs::File;
use std::io::Write;

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

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SoundeoTrackFullInfo {
    pub id: String,
    pub title: String,
    pub cover: String,
    #[serde(rename = "trackUrl")]
    #[serde(deserialize_with = "parse_soundeo_url")]
    pub track_url: String,
    pub release: String,
    pub label: String,
    pub genre: String,
    pub date: String,
    // #[serde(deserialize_with = "deserialize_to_number")]
    pub bpm: String,
    pub key: String,
    #[serde(rename(deserialize = "format2size"))]
    pub size: Option<String>,
    pub downloadable: bool,
    #[serde(default)]
    pub already_downloaded: bool,
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
            bpm: "".to_string(),
            key: "".to_string(),
            size: Some("".to_string()),
            downloadable: false,
            already_downloaded: false,
        }
    }
    pub async fn get_info(&mut self, soundeo_user: &SoundeoUser) -> SoundeoResult<()> {
        let mut log = DjWizardLog::read_log().change_context(SoundeoError)?;
        return match log.soundeo.tracks_info.get(&self.id) {
            Some(full_info) => {
                self.clone_from(full_info);
                Ok(())
            }
            None => {
                println!("Getting info for track {}", self.id.clone().cyan());
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
                log.soundeo
                    .tracks_info
                    .insert(self.id.clone(), full_info.clone());
                log.save_log(&soundeo_user).change_context(SoundeoError)?;
                println!(
                    "Track info successfully stored: {}",
                    self.title.clone().green()
                );
                Ok(())
            }
        };
    }

    async fn get_download_url(&self, soundeo_user: &mut SoundeoUser) -> SoundeoResult<String> {
        let response_text = SoundeoAPI::GetTrackDownloadUrl {
            track_id: self.id.clone(),
        }
        .get(soundeo_user)
        .await
        .change_context(SoundeoError)?;
        let json_resp: Value = serde_json::from_str(&response_text)
            .into_report()
            .change_context(SoundeoError)?;
        let download_url = json_resp["jsActions"]["redirect"]["url"]
            .clone()
            .to_string();
        soundeo_user
            .login_and_update_user_info()
            .await
            .change_context(SoundeoError)?;
        Ok(download_url)
    }

    pub async fn download_track(&mut self, soundeo_user: &mut SoundeoUser) -> SoundeoResult<()> {
        let download_url = self.get_download_url(soundeo_user).await?;
        let remaining_downloads = soundeo_user.get_remamining_downloads_string();
        println!("{}", remaining_downloads);
        let mut downloaded: u64 = 0;
        let client = reqwest::Client::new();
        let response = client
            .get(download_url)
            .send()
            .await
            .into_report()
            .change_context(SoundeoError)?;

        let total_size = response.content_length().unwrap();
        let file_name = self.get_file_name();
        let pb = ProgressBar::new(total_size);
        pb.set_style(ProgressStyle::default_bar()
            .template("{msg}\n{spinner:.green} [{elapsed_precise}] [{wide_bar:.white/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta})").into_report().change_context(SoundeoError)?
            .progress_chars("█  "));
        pb.set_message(format!(
            "Downloading {}, track id: {}",
            file_name.clone().cyan(),
            self.id.clone().cyan()
        ));

        let mut stream = response.bytes_stream();
        let file_path = format!("{}/{}", soundeo_user.download_path, file_name);
        let mut dest = File::create(file_path.clone())
            .into_report()
            .change_context(SoundeoError)?;

        while let Some(item) = stream.next().await {
            let chunk = item.into_report().change_context(SoundeoError)?;
            dest.write(&chunk)
                .into_report()
                .change_context(SoundeoError)?;
            let new = min(downloaded + (chunk.len() as u64), total_size);
            downloaded = new;
            pb.set_position(new);
        }
        self.already_downloaded = true;
        let message = format!("{} successfully downloaded", file_name.clone().green());
        pb.finish_with_message(message);
        Ok(())
    }

    fn get_file_name(&self) -> String {
        format!("{}.AIFF", self.title)
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
