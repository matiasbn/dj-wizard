use std::fmt;

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
