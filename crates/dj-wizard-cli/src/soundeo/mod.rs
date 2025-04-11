use std::collections::HashMap;
use std::fmt;

use colorize::AnsiColor;
use error_stack::ResultExt;
use serde::{Deserialize, Serialize};

use crate::log::{DjWizardLog, DjWizardLogResult};
use crate::soundeo::track::SoundeoTrack;
use crate::user::SoundeoUser;

pub mod api;
pub mod search_bar;
pub mod track;
pub mod track_list;

#[derive(Debug)]
pub struct SoundeoError;

impl fmt::Display for SoundeoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Soundeo error")
    }
}

impl std::error::Error for SoundeoError {}

pub type SoundeoResult<T> = error_stack::Result<T, SoundeoError>;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Soundeo {
    pub tracks_info: HashMap<String, SoundeoTrack>,
}

impl Soundeo {
    pub fn new() -> Self {
        Self {
            tracks_info: HashMap::new(),
        }
    }
}

pub trait SoundeoCRUD {
    fn create_soundeo_track(soundeo_track: SoundeoTrack) -> DjWizardLogResult<()>;

    fn mark_track_as_downloaded(soundeo_track_id: String) -> DjWizardLogResult<()>;
    fn reset_track_already_downloaded(soundeo_track_id: String) -> DjWizardLogResult<()>;
}
