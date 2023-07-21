use std::collections::{HashMap, HashSet};
use std::fs::{read_to_string, File};
use std::io::Cursor;
use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use std::{fmt, fs};

use colored::Colorize;
use error_stack::{IntoReport, ResultExt};
use reqwest::blocking::multipart::Form;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::soundeo::track::SoundeoTrack;
use crate::soundeo::Soundeo;
use crate::spotify::Spotify;
use crate::user::{IPFSConfig, SoundeoUser, User};

#[derive(Debug)]
pub struct DjWizardLogError;
impl fmt::Display for DjWizardLogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Dj Wizard log error")
    }
}
impl std::error::Error for DjWizardLogError {}

pub type DjWizardLogResult<T> = error_stack::Result<T, DjWizardLogError>;

#[derive(Debug, Serialize, Deserialize)]
pub struct DjWizardLog {
    pub last_update: u64,
    pub queued_tracks: HashSet<String>,
    pub spotify: Spotify,
    pub soundeo: Soundeo,
}

impl DjWizardLog {
    pub fn read_log() -> DjWizardLogResult<Self> {
        let soundeo_user = SoundeoUser::new().change_context(DjWizardLogError)?;
        let soundeo_log_path = Self::get_log_path(&soundeo_user);
        let soundeo_log_file_path = Path::new(&soundeo_log_path);
        let soundeo_log: Self = if soundeo_log_file_path.is_file() {
            let soundeo_log: Self = serde_json::from_str(
                &read_to_string(&soundeo_log_file_path)
                    .into_report()
                    .change_context(DjWizardLogError)?,
            )
            .into_report()
            .change_context(DjWizardLogError)?;
            soundeo_log
        } else {
            Self {
                last_update: 0,
                queued_tracks: HashSet::new(),
                soundeo: Soundeo::new(),
                spotify: Spotify::new(),
            }
        };
        Ok(soundeo_log)
    }

    pub fn save_log(&self, soundeo_user: &SoundeoUser) -> DjWizardLogResult<()> {
        let save_log_string = serde_json::to_string_pretty(self)
            .into_report()
            .change_context(DjWizardLogError)?;
        let log_path = Self::get_log_path(soundeo_user);
        fs::write(log_path, &save_log_string)
            .into_report()
            .change_context(DjWizardLogError)?;
        Ok(())
    }

    fn get_log_path(soundeo_user: &SoundeoUser) -> String {
        format!("{}/soundeo_log.json", soundeo_user.download_path)
    }

    pub fn mark_track_as_downloaded(&mut self, track_id: String) -> DjWizardLogResult<()> {
        self.last_update = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .into_report()
            .change_context(DjWizardLogError)?
            .as_secs();
        let track_info = self
            .soundeo
            .tracks_info
            .get_mut(&track_id)
            .ok_or(DjWizardLogError)
            .into_report()?;
        track_info.already_downloaded = true;
        Ok(())
    }

    pub fn write_queued_track_to_log(&mut self, track_id: String) -> DjWizardLogResult<bool> {
        self.last_update = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .into_report()
            .change_context(DjWizardLogError)?
            .as_secs();
        let result = self.queued_tracks.insert(track_id);
        Ok(result)
    }
    pub fn remove_queued_track_from_log(&mut self, track_id: String) -> DjWizardLogResult<bool> {
        self.last_update = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .into_report()
            .change_context(DjWizardLogError)?
            .as_secs();
        let result = self.queued_tracks.remove(&track_id);
        Ok(result)
    }

    pub fn upload_to_ipfs(&self) -> DjWizardLogResult<()> {
        println!("Saving the log file to IPFS");
        let soundeo_user = SoundeoUser::new().change_context(DjWizardLogError)?;
        let log_path = Self::get_log_path(&soundeo_user);
        let form = Form::new()
            .file("soundeo_log.json", log_path)
            .into_report()
            .change_context(DjWizardLogError)?;

        let mut config_file = User::new();
        config_file
            .read_config_file()
            .change_context(DjWizardLogError)?;
        let IPFSConfig {
            api_key,
            api_key_secret,
            ..
        } = config_file.ipfs.clone();

        let client = Client::builder()
            .timeout(Duration::from_secs(600))
            .build()
            .into_report()
            .change_context(DjWizardLogError)?;

        let response = client
            .post("https://ipfs.infura.io:5001/api/v0/add")
            .multipart(form)
            .basic_auth(api_key, Some(api_key_secret))
            .send()
            .into_report()
            .change_context(DjWizardLogError)?;
        let resp_text = response
            .text()
            .into_report()
            .change_context(DjWizardLogError)?;
        let value: Value = serde_json::from_str(&resp_text)
            .into_report()
            .change_context(DjWizardLogError)?;
        let hash = value["Hash"].clone().as_str().unwrap().to_string();
        config_file.ipfs.last_ipfs_hash = hash.clone();
        config_file
            .save_config_file()
            .change_context(DjWizardLogError)?;
        println!(
            "Log file successfully stored to IPFS with hash {}",
            hash.green()
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_upload_to_ipfs() {
        let mut log = DjWizardLog::read_log().unwrap();
        log.upload_to_ipfs().unwrap();
    }
}
