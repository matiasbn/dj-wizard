use crate::dialoguer::Dialoguer;
use crate::soundeo_log::DjWizardLog;
use colorize::AnsiColor;
use error_stack::{IntoReport, ResultExt};
use inflector::Inflector;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use strum::IntoEnumIterator;
use url::Url;

use crate::spotify::playlist::SpotifyPlaylist;
use crate::user::SoundeoUser;

pub mod api;
pub mod playlist;
pub mod track;

#[derive(Debug)]
pub struct SpotifyError;

impl fmt::Display for SpotifyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Spotify error")
    }
}

impl std::error::Error for SpotifyError {}

pub type SpotifyResult<T> = error_stack::Result<T, SpotifyError>;

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Spotify {
    pub playlists: HashMap<String, SpotifyPlaylist>,
}

impl Spotify {
    pub fn new() -> Self {
        Self {
            playlists: HashMap::new(),
        }
    }
}
#[derive(Debug, Deserialize, Serialize, Clone, strum_macros::Display, strum_macros::EnumIter)]
pub enum SpotifyCommands {
    AddNewPlaylist,
    UpdatePlaylist,
    DownloadTracksFromPlaylist,
}

impl SpotifyCommands {
    pub async fn execute() -> SpotifyResult<()> {
        let options = Self::get_options();
        let selection =
            Dialoguer::select("Select".to_string(), options, None).change_context(SpotifyError)?;
        return match Self::get_selection(selection) {
            SpotifyCommands::AddNewPlaylist => Self::add_new_playlist().await,
            SpotifyCommands::UpdatePlaylist => {
                unimplemented!()
            }
            SpotifyCommands::DownloadTracksFromPlaylist => {
                unimplemented!()
            }
        };
    }

    fn get_options() -> Vec<String> {
        Self::iter()
            .map(|element| element.to_string().to_sentence_case())
            .collect::<Vec<_>>()
    }

    fn get_selection(selection: usize) -> Self {
        match selection {
            0 => Self::AddNewPlaylist,
            _ => unimplemented!(),
        }
    }

    async fn add_new_playlist() -> SpotifyResult<()> {
        let prompt_text = format!("Spotify playlist url: ");
        let url = Dialoguer::input(prompt_text).change_context(SpotifyError)?;
        let playlist_url = Url::parse(&url)
            .into_report()
            .change_context(SpotifyError)?;

        let mut playlist =
            SpotifyPlaylist::new(playlist_url.to_string()).change_context(SpotifyError)?;
        playlist
            .get_playlist_info()
            .await
            .change_context(SpotifyError)?;
        let mut dj_wizard_log = DjWizardLog::read_log().change_context(SpotifyError)?;
        dj_wizard_log
            .spotify
            .playlists
            .insert(playlist.spotify_playlist_id.clone(), playlist.clone());
        let soundeo_user = SoundeoUser::new().change_context(SpotifyError)?;
        dj_wizard_log
            .save_log(&soundeo_user)
            .change_context(SpotifyError)?;
        println!(
            "Playlist {} successfully stored",
            playlist.name.clone().green()
        );
        Ok(())
    }
}
