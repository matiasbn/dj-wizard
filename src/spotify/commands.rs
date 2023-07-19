use colorize::AnsiColor;
use error_stack::{IntoReport, ResultExt};
use inflector::Inflector;
use serde::{Deserialize, Serialize};
use strum::IntoEnumIterator;
use url::Url;

use crate::dialoguer::Dialoguer;
use crate::soundeo_log::DjWizardLog;
use crate::spotify::playlist::SpotifyPlaylist;
use crate::spotify::{SpotifyError, SpotifyResult};
use crate::user::SoundeoUser;

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
            SpotifyCommands::DownloadTracksFromPlaylist => Self::download_from_playlist().await,
        };
    }

    fn get_options() -> Vec<String> {
        Self::iter()
            .map(|element| element.to_string().to_sentence_case())
            .collect::<Vec<_>>()
    }

    fn get_selection(selection: usize) -> Self {
        let options = Self::iter().collect::<Vec<_>>();
        options[selection].clone()
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

    async fn download_from_playlist() -> SpotifyResult<()> {
        let log = DjWizardLog::read_log().change_context(SpotifyError)?;
        let playlist_names = log
            .spotify
            .playlists
            .values()
            .map(|playlist| playlist.name.clone())
            .collect::<Vec<_>>();
        let prompt_text = "Select the playlist to download";
        let selection = Dialoguer::select(prompt_text.to_string(), playlist_names.clone(), None)
            .change_context(SpotifyError)?;
        let playlist = log
            .spotify
            .get_playlist_by_name(playlist_names[selection].clone())?;
        println!("{:#?}", playlist);
        Ok(())
    }
}
