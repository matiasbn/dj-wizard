use colored::Colorize;
use colorize::AnsiColor;
use error_stack::{IntoReport, Report, ResultExt};
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
use crate::{DjWizardCommands, Suggestion};

#[derive(Debug, Deserialize, Serialize, Clone, strum_macros::Display, strum_macros::EnumIter)]
pub enum SpotifyCommands {
    AddNewPlaylist,
    UpdatePlaylist,
    DownloadTracksFromPlaylist,
    // CreateSpotifyPlaylistFile,
    PrintDownloadedTracksByPlaylist,
}

impl SpotifyCommands {
    pub async fn execute() -> SpotifyResult<()> {
        let options = Self::get_options();
        let selection =
            Dialoguer::select("Select".to_string(), options, None).change_context(SpotifyError)?;
        return match Self::get_selection(selection) {
            SpotifyCommands::AddNewPlaylist => Self::add_new_playlist().await,
            SpotifyCommands::UpdatePlaylist => Self::update_playlist().await,
            SpotifyCommands::DownloadTracksFromPlaylist => Self::download_from_playlist().await,
            SpotifyCommands::PrintDownloadedTracksByPlaylist => {
                Self::print_downloaded_songs_by_playlist()
            }
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

        // check if already added
        let spotify = DjWizardLog::get_spotify().change_context(SpotifyError)?;
        let stored_playlist = spotify.playlists.get(&playlist.spotify_playlist_id.clone());

        return match stored_playlist {
            Some(stored) => {
                return Err(Report::new(SpotifyError)
                    .attach_printable(format!(
                        "Spotify playlist {} already added",
                        stored.name.clone().yellow()
                    ))
                    .attach(Suggestion(format!(
                        "Update the playlist by running {} and update selecting the correct option",
                        DjWizardCommands::Spotify.cli_command().yellow()
                    ))));
            }
            None => {
                playlist
                    .get_playlist_info()
                    .await
                    .change_context(SpotifyError)?;
                DjWizardLog::create_spotify_playlist(playlist.clone())
                    .change_context(SpotifyError)?;
                println!(
                    "Playlist {} successfully stored",
                    playlist.name.clone().green()
                );
                Ok(())
            }
        };
    }

    async fn update_playlist() -> SpotifyResult<()> {
        let mut playlist =
            SpotifyPlaylist::prompt_select_playlist("Select the playlist to download")?;
        playlist
            .get_playlist_info()
            .await
            .change_context(SpotifyError)?;
        DjWizardLog::create_spotify_playlist(playlist.clone()).change_context(SpotifyError)?;
        println!(
            "Playlist {} successfully updated",
            playlist.name.clone().green()
        );
        Ok(())
    }

    async fn download_from_playlist() -> SpotifyResult<()> {
        let spotify = DjWizardLog::get_spotify().change_context(SpotifyError)?;
        let playlist = SpotifyPlaylist::prompt_select_playlist("Select the playlist to download")?;
        let mut soundeo_user = SoundeoUser::new().change_context(SpotifyError)?;
        soundeo_user
            .login_and_update_user_info()
            .await
            .change_context(SpotifyError)?;
        let mut soundeo_ids = vec![];
        for (spotify_track_id, mut spotify_track) in playlist.tracks {
            let soundeo_track_id = if let Some(soundeo_track_id) =
                spotify.soundeo_track_ids.get(&spotify_track_id)
            {
                soundeo_track_id.clone()
            } else {
                let soundeo_track_id = spotify_track.get_soundeo_track_id(&soundeo_user).await?;
                DjWizardLog::update_spotify_to_soundeo_track(
                    spotify_track_id.clone(),
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

    fn print_downloaded_songs_by_playlist() -> SpotifyResult<()> {
        let playlist = SpotifyPlaylist::prompt_select_playlist("Select the playlist to print")?;
        let spotify = DjWizardLog::get_spotify().change_context(SpotifyError)?;
        let spotify_mapped_tracks = spotify
            .soundeo_track_ids
            .into_iter()
            .filter(|(spotify_id, _)| playlist.tracks.contains_key(spotify_id))
            .filter_map(|(_, soundeo_id)| soundeo_id)
            .collect::<Vec<_>>();
        let soundeo = DjWizardLog::get_soundeo().change_context(SpotifyError)?;
        let mut downloaded_tracks = soundeo
            .tracks_info
            .into_iter()
            .filter(|(soundeo_track_id, _)| {
                spotify_mapped_tracks.contains(&soundeo_track_id.clone())
            })
            .collect::<Vec<_>>();
        downloaded_tracks.sort_by_key(|(_, soundeo_track)| soundeo_track.title.clone());
        println!(
            "Playlist {} has {} tracks, {} were already downloaded",
            playlist.name.green(),
            format!("{}", playlist.tracks.len()).green(),
            format!("{}", downloaded_tracks.len()).green(),
        );
        println!("Downloaded tracks sorted by artist name:");
        for (_, soundeo_track) in downloaded_tracks {
            println!(
                "{}: {}",
                soundeo_track.title.clone().green(),
                soundeo_track.clone().get_track_url().cyan()
            );
        }
        Ok(())
    }

    fn create_spotify_playlist_file() -> SpotifyResult<()> {
        let prompt_text = "Select the playlist to create the m3u8 file";
        let playlist = SpotifyPlaylist::prompt_select_playlist(prompt_text)?;
        let mut file_content = "#EXTM3U";

        Ok(())
    }
}
