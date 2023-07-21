use std::collections::HashMap;
use std::fmt;

use colorize::AnsiColor;
use error_stack::ResultExt;
use serde::{Deserialize, Serialize};

use crate::log::DjWizardLog;
use crate::soundeo::full_info::SoundeoTrackFullInfo;
use crate::user::SoundeoUser;

pub mod api;
pub mod full_info;
pub mod search_bar;
pub mod track;

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
    pub tracks_info: HashMap<String, SoundeoTrackFullInfo>,
}

impl Soundeo {
    pub fn new() -> Self {
        Self {
            tracks_info: HashMap::new(),
        }
    }
}
