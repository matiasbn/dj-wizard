pub mod commands;

use ::serde::{Deserialize, Serialize};
use std::fmt;
use url::Url;

use crate::log::DjWizardLogResult;

#[derive(Debug)]
pub struct UrlListError;

impl fmt::Display for UrlListError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Url error")
    }
}

impl std::error::Error for UrlListError {}

pub type UrlListResult<T> = error_stack::Result<T, UrlListError>;

pub trait UrlListCRUD {
    fn add_url_to_url_list(soundeo_url: Url) -> DjWizardLogResult<bool>;
}
