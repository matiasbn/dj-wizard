use std::collections::{HashMap, HashSet};
use std::fs::read_to_string;
use std::path::Path;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use std::{fmt, fs};

use error_stack::{IntoReport, ResultExt};
use serde::{Deserialize, Serialize};

use crate::track::SoundeoTrack;
use crate::user::SoundeoUser;

#[derive(Debug)]
pub struct SoundeoBotLogError;
impl fmt::Display for SoundeoBotLogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SoundeoBotLog error")
    }
}
impl std::error::Error for SoundeoBotLogError {}

pub type SoundeoBotLogResult<T> = error_stack::Result<T, SoundeoBotLogError>;

#[derive(Debug, Serialize, Deserialize)]
pub struct SoundeoBotLog {
    pub path: String,
    pub last_update: u64,
    pub downloaded_tracks: HashMap<String, SoundeoTrack>,
    pub queued_tracks: HashSet<String>,
}

impl SoundeoBotLog {
    pub fn read_log() -> SoundeoBotLogResult<Self> {
        let soundeo_user = SoundeoUser::new().change_context(SoundeoBotLogError)?;
        let soundeo_log_path = Self::get_log_path(&soundeo_user);
        let soundeo_log_file_path = Path::new(&soundeo_log_path);
        let soundeo_log: Self = if soundeo_log_file_path.is_file() {
            let soundeo_log: Self = serde_json::from_str(
                &read_to_string(&soundeo_log_file_path)
                    .into_report()
                    .change_context(SoundeoBotLogError)?,
            )
            .into_report()
            .change_context(SoundeoBotLogError)?;
            soundeo_log
        } else {
            Self {
                path: soundeo_log_file_path.to_str().unwrap().to_string(),
                last_update: 0,
                downloaded_tracks: HashMap::new(),
                queued_tracks: HashSet::new(),
            }
        };
        Ok(soundeo_log)
    }

    pub fn save_log(&self, soundeo_user: &SoundeoUser) -> SoundeoBotLogResult<()> {
        let save_log_string = serde_json::to_string_pretty(self)
            .into_report()
            .change_context(SoundeoBotLogError)?;
        let log_path = Self::get_log_path(soundeo_user);
        fs::write(log_path, &save_log_string)
            .into_report()
            .change_context(SoundeoBotLogError)?;
        Ok(())
    }

    fn get_log_path(soundeo_user: &SoundeoUser) -> String {
        format!("{}/soundeo_log.json", soundeo_user.download_path)
    }

    pub fn write_downloaded_track_to_log(
        &mut self,
        track: SoundeoTrack,
    ) -> SoundeoBotLogResult<()> {
        self.last_update = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .into_report()
            .change_context(SoundeoBotLogError)?
            .as_secs();
        self.downloaded_tracks
            .insert(track.track_id.clone(), track.clone());
        Ok(())
    }

    pub fn write_queued_track_to_log(&mut self, track_id: String) -> SoundeoBotLogResult<bool> {
        self.last_update = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .into_report()
            .change_context(SoundeoBotLogError)?
            .as_secs();
        let result = self.queued_tracks.insert(track_id);
        Ok(result)
    }
    pub fn remove_queued_track_from_log(&mut self, track_id: String) -> SoundeoBotLogResult<bool> {
        self.last_update = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .into_report()
            .change_context(SoundeoBotLogError)?
            .as_secs();
        let result = self.queued_tracks.remove(&track_id);
        Ok(result)
    }
}
