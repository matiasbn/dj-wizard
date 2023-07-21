use std::collections::{HashMap, HashSet};
use std::fs::{read_to_string, File};
use std::io::Cursor;
use std::path::Path;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use std::{fmt, fs};

use crate::soundeo::full_info::SoundeoTrackFullInfo;
use crate::soundeo::track::SoundeoTrack;
use crate::soundeo::Soundeo;
use crate::spotify::Spotify;
use error_stack::{IntoReport, ResultExt};
use reqwest::blocking::multipart::Form;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::user::SoundeoUser;

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
    pub path: String,
    pub last_update: u64,
    pub downloaded_tracks: HashMap<String, SoundeoTrack>,
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
                path: soundeo_log_file_path.to_str().unwrap().to_string(),
                last_update: 0,
                downloaded_tracks: HashMap::new(),
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

    pub fn write_downloaded_track_to_log(&mut self, track: SoundeoTrack) -> DjWizardLogResult<()> {
        self.last_update = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .into_report()
            .change_context(DjWizardLogError)?
            .as_secs();
        self.downloaded_tracks
            .insert(track.track_id.clone(), track.clone());
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

    pub fn upload_to_ipfs(&mut self) -> DjWizardLogResult<()> {
        let soundeo_user = SoundeoUser::new().change_context(DjWizardLogError)?;
        let log_path = Self::get_log_path(&soundeo_user);
        let form = Form::new()
            .file("soundeo_log.json", log_path)
            .into_report()
            .change_context(DjWizardLogError)?;
        let file = File::open(Self::get_log_path(&soundeo_user))
            .into_report()
            .change_context(DjWizardLogError)?;
        let metda = file.metadata().unwrap();
        println!("{:#?}", metda);

        let client = Client::new();
        let response = client
            .post("https://ipfs.infura.io:5001/api/v0/add")
            .multipart(form)
            .basic_auth(api_key, Some(api_secret))
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
        println!("{}", hash);
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
