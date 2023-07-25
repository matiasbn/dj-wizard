use colorize::AnsiColor;
use error_stack::{IntoReport, ResultExt};
use inflector::Inflector;
use serde::{Deserialize, Serialize};
use strum::IntoEnumIterator;
use url::Url;

use crate::dialoguer::Dialoguer;
use crate::log::DjWizardLog;
use crate::soundeo::track::SoundeoTrack;
use crate::spotify::playlist::SpotifyPlaylist;
use crate::spotify::{SpotifyCRUD, SpotifyError, SpotifyResult};
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
        DjWizardLog::create_spotify_playlist(playlist.clone()).change_context(SpotifyError)?;
        println!(
            "Playlist {} successfully stored",
            playlist.name.clone().green()
        );
        Ok(())
    }

    async fn download_from_playlist() -> SpotifyResult<()> {
        let mut spotify = DjWizardLog::get_spotify().change_context(SpotifyError)?;
        let playlist_names = spotify
            .playlists
            .values()
            .map(|playlist| playlist.name.clone())
            .collect::<Vec<_>>();
        let prompt_text = "Select the playlist to download";
        let selection = Dialoguer::select(prompt_text.to_string(), playlist_names.clone(), None)
            .change_context(SpotifyError)?;
        let playlist = spotify.get_playlist_by_name(playlist_names[selection].clone())?;
        let mut soundeo_user = SoundeoUser::new().change_context(SpotifyError)?;
        soundeo_user
            .login_and_update_user_info()
            .await
            .change_context(SpotifyError)?;
        let mut soundeo_ids = vec![];
        for (spotify_id, mut track) in playlist.tracks {
            let soundeo_track_id =
                if let Some(soundeo_track_id) = spotify.soundeo_track_ids.get(&spotify_id) {
                    soundeo_track_id.clone()
                } else {
                    let soundeo_track_id = track.get_soundeo_track_id(&soundeo_user).await?;
                    DjWizardLog::update_spotify_to_soundeo_list(
                        spotify_id.clone(),
                        soundeo_track_id.clone(),
                    )
                    .change_context(SpotifyError)?;
                    soundeo_track_id.clone()
                };
            if soundeo_track_id.is_some() {
                soundeo_ids.push(soundeo_track_id.clone().unwrap());
            }
        }
        for soundeo_track_id in soundeo_ids {
            let mut track = SoundeoTrack::new(soundeo_track_id);
            track
                .download_track(&mut soundeo_user)
                .await
                .change_context(SpotifyError)?;
        }
        Ok(())
    }
}
