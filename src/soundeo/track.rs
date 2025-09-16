use colored::Colorize;
use std::cmp::min;
use std::fs::File;
use std::io::Write;

use colorize::AnsiColor;
use error_stack::{IntoReport, Report, ResultExt};
use futures_util::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use crate::log::DjWizardLog;
use crate::soundeo::api::SoundeoAPI;
use crate::soundeo::{SoundeoCRUD, SoundeoError, SoundeoResult};
use crate::user::SoundeoUser;
use crate::Suggestion;

pub fn deserialize_to_number<'de, D>(deserializer: D) -> Result<u32, D::Error>
where
    D: Deserializer<'de>,
{
    let s: String = String::deserialize(deserializer)?;
    s.parse::<u32>().map_err(serde::de::Error::custom)
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SoundeoTrack {
    pub id: String,
    pub title: String,
    #[serde(rename = "trackUrl")]
    pub track_url: String,
    pub release: String,
    pub label: String,
    pub genre: String,
    pub date: String,
    // #[serde(deserialize_with = "deserialize_to_number")]
    pub bpm: String,
    pub key: Option<String>,
    #[serde(rename(deserialize = "format2size"))]
    pub size: Option<String>,
    pub downloadable: bool,
    #[serde(default)]
    pub already_downloaded: bool,
    #[serde(default)]
    pub migrated: bool,
}

impl SoundeoTrack {
    pub fn new(id: String) -> Self {
        SoundeoTrack {
            id,
            title: "".to_string(),
            track_url: "".to_string(),
            release: "".to_string(),
            label: "".to_string(),
            genre: "".to_string(),
            date: "".to_string(),
            bpm: "".to_string(),
            key: Some("".to_string()),
            size: Some("".to_string()),
            downloadable: false,
            already_downloaded: false,
            migrated: false,
        }
    }
    pub async fn get_info(&mut self, soundeo_user: &SoundeoUser, print: bool) -> SoundeoResult<()> {
        // Try Firebase first (if available), fallback to local JSON
        if let Ok(firebase_track) = self.get_info_from_firebase().await {
            if let Some(full_info) = firebase_track {
                self.clone_from(&full_info);
                return Ok(());
            }
        }
        
        // Fallback to local JSON
        let soundeo = DjWizardLog::get_soundeo().change_context(SoundeoError)?;
        return match soundeo.tracks_info.get(&self.id) {
            Some(full_info) => {
                self.clone_from(full_info);
                Ok(())
            }
            None => {
                if print {
                    println!("Getting info for track {}", self.id.clone().cyan());
                }
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
                DjWizardLog::create_soundeo_track(full_info.clone())
                    .change_context(SoundeoError)?;
                if print {
                    println!(
                        "Track info successfully stored: {}",
                        self.title.clone().green()
                    );
                }
                Ok(())
            }
        };
    }

    pub async fn get_download_url(&self, soundeo_user: &mut SoundeoUser) -> SoundeoResult<String> {
        let response_text = SoundeoAPI::GetTrackDownloadUrl {
            track_id: self.id.clone(),
        }
        .get(soundeo_user)
        .await
        .change_context(SoundeoError)?;
        let json_resp: Value = serde_json::from_str(&response_text)
            .into_report()
            .change_context(SoundeoError)?;
        let js_actions = json_resp["jsActions"]
            .as_object()
            .ok_or(SoundeoError)
            .into_report()?;
        match js_actions.get("flash") {
            None => {
                let mut download_url = js_actions["redirect"]["url"].clone().to_string();
                download_url = download_url
                    .trim_end_matches("\"")
                    .trim_start_matches("\"")
                    .to_string();
                soundeo_user
                    .login_and_update_user_info()
                    .await
                    .change_context(SoundeoError)?;
                Ok(download_url)
            }
            Some(flash_object) => {
                // No more downloads available
                let flash = flash_object.as_object().ok_or(SoundeoError).into_report()?;
                let message = flash.get("message").unwrap().to_string();
                return Err(Report::new(SoundeoError).attach(Suggestion(message)));
            }
        }
    }

    async fn get_info_from_firebase(&self) -> SoundeoResult<Option<SoundeoTrack>> {
        // Try to get Firebase client and fetch track
        use crate::auth::{firebase_client::FirebaseClient, google_auth::GoogleAuth};
        
        // Load auth token
        let auth_token = match GoogleAuth::load_token().await {
            Ok(token) => token,
            Err(_) => return Ok(None), // No auth, fallback to local
        };
        
        // Create Firebase client
        let firebase_client = match FirebaseClient::new(auth_token).await {
            Ok(client) => client,
            Err(_) => return Ok(None), // Firebase unavailable, fallback to local
        };
        
        // Get track from Firebase
        match firebase_client.get_track(&self.id).await {
            Ok(track) => Ok(track),
            Err(_) => Ok(None), // Track not in Firebase, fallback to local
        }
    }

    pub async fn reset_already_downloaded(
        &mut self,
        soundeo_user: &mut SoundeoUser,
    ) -> SoundeoResult<()> {
        // Mark as not downloaded
        DjWizardLog::reset_track_already_downloaded(self.id.clone())
            .change_context(SoundeoError)?;
        soundeo_user
            .login_and_update_user_info()
            .await
            .change_context(SoundeoError)?;
        Ok(())
    }

    pub async fn download_track(
        &mut self,
        soundeo_user: &mut SoundeoUser,
        print_remaining_downloads: bool,
        force_redownload: bool,
    ) -> SoundeoResult<()> {
        // Get info
        self.get_info(&soundeo_user, true).await?;
        // Check if can be downloaded
        if self.already_downloaded {
            if force_redownload {
                self.print_downloading_again();
            } else {
                self.print_already_downloaded();
                return Ok(());
            }
        }

        if !self.downloadable {
            self.print_not_downloadable();
            return Ok(());
        }

        // Download
        let download_url = self.get_download_url(soundeo_user).await?;
        if print_remaining_downloads {
            let remaining_downloads = soundeo_user.get_remamining_downloads_string();
            println!("{}", remaining_downloads);
        }
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
        let file_path = format!(
            "{}/{}",
            soundeo_user.download_path,
            file_name.replace("/", ",")
        );
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
        let message = format!("{} successfully downloaded", file_name.clone().green());
        pb.finish_with_message(message);

        // Mark as downloaded
        DjWizardLog::mark_track_as_downloaded(self.id.clone()).change_context(SoundeoError)?;
        soundeo_user
            .login_and_update_user_info()
            .await
            .change_context(SoundeoError)?;
        Ok(())
    }

    pub fn get_track_url(&self) -> String {
        format!("https://www.soundeo.com/{}", self.track_url)
    }

    pub fn print_already_downloaded(&self) {
        println!(
            "Track already downloaded: {},  {}",
            self.title.clone().yellow(),
            self.get_track_url().yellow()
        );
    }

    pub fn print_downloading_again(&self) {
        println!(
            "Downloading track again: {},  {}",
            self.title.clone().purple(),
            self.get_track_url().purple()
        );
    }

    pub fn print_already_available(&self) {
        println!(
            "Track {} (ID:{}) is already available for download, skipping",
            self.title.clone().yellow(),
            self.id.clone().yellow(),
        );
    }

    pub fn print_not_downloadable(&self) {
        println!(
            "Track isn't downloadable: {}, {}",
            self.title.clone().yellow(),
            self.get_track_url().yellow()
        );
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
        let track_id = "3834116".to_string();
        let mut soundeo_full_info = SoundeoTrack::new(track_id);
        let mut soundeo_user = SoundeoUser::new().unwrap();
        soundeo_user.login_and_update_user_info().await.unwrap();
        soundeo_full_info
            .get_info(&soundeo_user, true)
            .await
            .unwrap();
        println!("{:#?}", soundeo_full_info);
    }

    #[tokio::test]
    async fn test_get_track() {
        let track_id = "12285307".to_string();
        let mut track = SoundeoTrack::new(track_id);
        let mut soundeo_user = SoundeoUser::new().unwrap();
        soundeo_user.login_and_update_user_info().await.unwrap();
        track
            .download_track(&mut soundeo_user, true, false)
            .await
            .unwrap();
        println!("{:#?}", track);
    }
}
