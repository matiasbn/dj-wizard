use base64::{engine::general_purpose, Engine as _};
use colored::Colorize;
use error_stack::{IntoReport, Report, ResultExt};
use inflector::Inflector;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use strum::IntoEnumIterator;
use tiny_http::{Response, Server};
use url::Url;
use webbrowser;

use crate::config::AppConfig;
use crate::dialoguer::Dialoguer;
use crate::log::DjWizardLog;
use crate::soundeo::track::SoundeoTrack;
use crate::spotify::playlist::SpotifyPlaylist;
use crate::spotify::{SpotifyCRUD, SpotifyError, SpotifyResult};
use crate::user::{SoundeoUser, User};
use crate::{DjWizardCommands, Suggestion};

#[derive(Debug, Deserialize, Serialize, Clone)]
struct TracksInfo {
    href: String,
    total: u32,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct ApiSimplePlaylist {
    id: String,
    name: String,
    public: bool,
    href: String, // API URL for the full playlist object
    tracks: TracksInfo,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct PaginatedPlaylistsResponse {
    items: Vec<ApiSimplePlaylist>,
    next: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone, strum_macros::Display, strum_macros::EnumIter)]
pub enum SpotifyCommands {
    SyncPublicPlaylists,
    DownloadTracksFromPlaylist,
    AddNewPlaylistFromUrl,
    UpdatePlaylist,
    PrintDownloadedTracksByPlaylist,
    DeletePlaylists,
    CountQueuedTracksByPlaylist,
}

impl SpotifyCommands {
    pub async fn execute() -> SpotifyResult<()> {
        let mut user_config = User::new();
        user_config
            .read_config_file()
            .change_context(SpotifyError)?;

        if user_config.spotify_access_token.is_empty() {
            let wants_to_login = Dialoguer::confirm(
                "You are not logged into Spotify. Would you like to log in now?".to_string(),
                Some(true),
            )
            .change_context(SpotifyError)?;

            if wants_to_login {
                Self::perform_spotify_login(&mut user_config).await?;
            } else {
                println!(
                    "{}",
                    "Spotify commands cannot be used without logging in.".yellow()
                );
                return Ok(());
            }
        }

        let options = Self::get_options();
        let selection =
            Dialoguer::select("Select".to_string(), options, None).change_context(SpotifyError)?;
        return match Self::get_selection(selection) {
            SpotifyCommands::AddNewPlaylistFromUrl => {
                Self::add_new_playlist(&mut user_config).await
            }
            SpotifyCommands::UpdatePlaylist => Self::update_playlist(&mut user_config).await,
            SpotifyCommands::SyncPublicPlaylists => {
                Self::sync_public_playlists(&mut user_config).await
            }
            SpotifyCommands::DownloadTracksFromPlaylist => Self::download_from_playlist().await,
            SpotifyCommands::PrintDownloadedTracksByPlaylist => {
                Self::print_downloaded_songs_by_playlist()
            }
            SpotifyCommands::DeletePlaylists => Self::delete_playlists(),
            SpotifyCommands::CountQueuedTracksByPlaylist => Self::count_queued_tracks_by_playlist(),
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

    async fn add_new_playlist(user_config: &mut User) -> SpotifyResult<()> {
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
                    .get_playlist_info(user_config, true)
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

    async fn update_playlist(user_config: &mut User) -> SpotifyResult<()> {
        let mut playlist =
            SpotifyPlaylist::prompt_select_playlist("Select the playlist to download")?;
        playlist
            .get_playlist_info(user_config, true)
            .await
            .change_context(SpotifyError)?;
        DjWizardLog::create_spotify_playlist(playlist.clone()).change_context(SpotifyError)?;
        println!(
            "Playlist {} successfully updated",
            playlist.name.clone().green()
        );
        Ok(())
    }

    async fn sync_public_playlists(user_config: &mut User) -> SpotifyResult<()> {
        println!("Fetching your public playlists from Spotify...");
        let client = reqwest::Client::new();
        let mut all_playlists: Vec<ApiSimplePlaylist> = Vec::new();
        let mut next_url = Some("https://api.spotify.com/v1/me/playlists?limit=50".to_string());

        while let Some(url) = next_url {
            let mut response = client
                .get(&url)
                .bearer_auth(&user_config.spotify_access_token)
                .send()
                .await
                .into_report()
                .change_context(SpotifyError)?;

            if response.status() == reqwest::StatusCode::UNAUTHORIZED {
                user_config
                    .refresh_spotify_token()
                    .await
                    .change_context(SpotifyError)?;
                response = client
                    .get(&url)
                    .bearer_auth(&user_config.spotify_access_token)
                    .send()
                    .await
                    .into_report()
                    .change_context(SpotifyError)?;
            }

            if !response.status().is_success() {
                let error_body = response
                    .text()
                    .await
                    .unwrap_or_else(|_| "Could not read error body".to_string());
                return Err(Report::new(SpotifyError)
                    .attach_printable(format!("Spotify API returned an error: {}", error_body))
                    .attach(Suggestion(
                        "Your refresh token might be invalid. Please log in again.".to_string(),
                    )));
            }

            let paginated_response: PaginatedPlaylistsResponse = response
                .json()
                .await
                .into_report()
                .change_context(SpotifyError)?;

            all_playlists.extend(paginated_response.items);
            next_url = paginated_response.next;
        }

        let mut public_playlists: Vec<ApiSimplePlaylist> =
            all_playlists.iter().filter(|p| p.public).cloned().collect();

        // Sort playlists alphabetically for a better user experience
        public_playlists.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

        if public_playlists.is_empty() {
            println!("{}", "No public playlists found in your account.".yellow());
            return Ok(());
        }

        println!("Found {} public playlists.", public_playlists.len());

        let options = vec![
            "Sync all public playlists",
            "Select specific playlists to sync",
        ];
        let selection =
            Dialoguer::select("How would you like to sync?".to_string(), options, Some(0))
                .change_context(SpotifyError)?;

        let playlists_to_sync = match selection {
            0 => public_playlists,
            1 => {
                let playlist_names: Vec<String> =
                    public_playlists.iter().map(|p| p.name.clone()).collect();

                let selections = Dialoguer::multiselect(
                    "Select playlists to sync (space to select, enter to confirm)".to_string(),
                    playlist_names,
                    Some(&vec![false; public_playlists.len()]),
                    false,
                )
                .change_context(SpotifyError)?;

                if selections.is_empty() {
                    println!("No playlists selected. Operation cancelled.");
                    return Ok(());
                }

                selections
                    .into_iter()
                    .map(|i| public_playlists[i].clone())
                    .collect()
            }
            _ => unreachable!(),
        };

        if playlists_to_sync.is_empty() {
            println!("{}", "No playlists to sync.".yellow());
            return Ok(());
        }

        println!(
            "\nStarting sync for {} playlists...",
            playlists_to_sync.len()
        );

        let local_spotify_log = DjWizardLog::get_spotify().change_context(SpotifyError)?;

        for simple_playlist in playlists_to_sync {
            if local_spotify_log
                .playlists
                .contains_key(&simple_playlist.id)
            {
                println!(
                    "Playlist '{}' already exists locally. Skipping.",
                    simple_playlist.name.yellow()
                );
                continue;
            }

            println!(
                "Syncing new playlist: {}",
                simple_playlist.name.clone().green()
            );
            let playlist_url = format!("https://open.spotify.com/playlist/{}", simple_playlist.id);
            let mut playlist = SpotifyPlaylist::new(playlist_url).change_context(SpotifyError)?;

            playlist
                .get_playlist_info(user_config, false)
                .await
                .change_context(SpotifyError)?;

            DjWizardLog::create_spotify_playlist(playlist.clone()).change_context(SpotifyError)?;
        }

        // --- (Optional) Clean up stale playlists that were deleted from Spotify ---
        let local_log = DjWizardLog::get_spotify().change_context(SpotifyError)?;
        // We use all_playlists (public and private) to check for existence.
        let remote_playlist_ids: std::collections::HashSet<String> =
            all_playlists.iter().map(|p| p.id.clone()).collect();

        let stale_playlists: Vec<SpotifyPlaylist> = local_log
            .playlists
            .values()
            .filter(|local_playlist| {
                !remote_playlist_ids.contains(&local_playlist.spotify_playlist_id)
            })
            .cloned()
            .collect();

        if !stale_playlists.is_empty() {
            println!("\n{}", "The following playlists exist locally but were not found in your Spotify account (they may have been deleted):".yellow());
            for p in &stale_playlists {
                println!("- {}", p.name.red());
            }

            let confirm_delete = Dialoguer::confirm(
                "Do you want to remove these stale playlists from the local log?".to_string(),
                Some(true), // Default to yes, as they are likely deleted.
            )
            .change_context(SpotifyError)?;

            if confirm_delete {
                let ids_to_delete: Vec<String> = stale_playlists
                    .iter()
                    .map(|p| p.spotify_playlist_id.clone())
                    .collect();
                DjWizardLog::delete_spotify_playlists(&ids_to_delete)
                    .change_context(SpotifyError)?;
                println!("{}", "Stale playlists removed from the local log.".green());
            }
        }

        println!("\n{}", "Sync complete.".green());
        Ok(())
    }

    async fn download_from_playlist() -> SpotifyResult<()> {
        let mut playlist =
            SpotifyPlaylist::prompt_select_playlist("Select the playlist to download")?;
        let mut soundeo_user = SoundeoUser::new().change_context(SpotifyError)?;
        soundeo_user
            .login_and_update_user_info()
            .await
            .change_context(SpotifyError)?;

        // Ensure all tracks are paired before proceeding.
        playlist
            .pair_unpaired_tracks(&mut soundeo_user)
            .await
            .change_context(SpotifyError)?;

        let spotify_log = DjWizardLog::get_spotify().change_context(SpotifyError)?;
        let soundeo_ids: Vec<String> = playlist
            .tracks
            .keys()
            .filter_map(|spotify_id| spotify_log.soundeo_track_ids.get(spotify_id))
            .filter_map(|soundeo_id_option| soundeo_id_option.as_ref())
            .cloned()
            .collect();

        if soundeo_ids.is_empty() {
            println!(
                "{}",
                "No downloadable tracks found for this playlist after pairing.".yellow()
            );
            return Ok(());
        }

        println!(
            "Found {} matched tracks. Starting download...",
            soundeo_ids.len()
        );

        for soundeo_track_id in soundeo_ids {
            let mut track = SoundeoTrack::new(soundeo_track_id);
            track
                .download_track(&mut soundeo_user, true)
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

    fn delete_playlists() -> SpotifyResult<()> {
        let spotify_log = DjWizardLog::get_spotify().change_context(SpotifyError)?;
        if spotify_log.playlists.is_empty() {
            println!("{}", "No Spotify playlists found in the log.".yellow());
            return Ok(());
        }

        let mut playlists: Vec<_> = spotify_log.playlists.values().cloned().collect();
        playlists.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

        let playlist_names: Vec<String> = playlists.iter().map(|p| p.name.clone()).collect();

        let selections = Dialoguer::multiselect(
            "Select the playlists you want to delete from the local log (press space to select, enter to confirm)".to_string(),
            playlist_names,
            Some(&vec![false; playlists.len()]),
            false,
        )
        .change_context(SpotifyError)?;

        if selections.is_empty() {
            println!("No playlists selected. Operation cancelled.");
            return Ok(());
        }

        let playlists_to_delete: Vec<SpotifyPlaylist> =
            selections.iter().map(|&i| playlists[i].clone()).collect();

        println!("\nYou have selected the following playlists for deletion:");
        for p in &playlists_to_delete {
            println!("- {}", p.name.red());
        }

        let confirmation = Dialoguer::confirm(
            "Are you sure you want to permanently delete these playlists from the local log? This action cannot be undone.".to_string(),
            Some(false),
        ).change_context(SpotifyError)?;

        if confirmation {
            let ids_to_delete: Vec<String> = playlists_to_delete
                .iter()
                .map(|p| p.spotify_playlist_id.clone())
                .collect();
            DjWizardLog::delete_spotify_playlists(&ids_to_delete).change_context(SpotifyError)?;
            println!(
                "\n{}",
                "Selected playlists have been deleted successfully.".green()
            );
        } else {
            println!("Deletion cancelled.");
        }

        Ok(())
    }

    fn count_queued_tracks_by_playlist() -> SpotifyResult<()> {
        // 1. Get all necessary data from the log
        let spotify_log = DjWizardLog::get_spotify().change_context(SpotifyError)?;
        let queued_tracks = DjWizardLog::get_queued_tracks().change_context(SpotifyError)?;

        if spotify_log.playlists.is_empty() {
            println!(
                "{}",
                "No Spotify playlists found in the log. Sync or add some first.".yellow()
            );
            return Ok(());
        }

        if queued_tracks.is_empty() {
            println!("{}", "The download queue is currently empty.".yellow());
            return Ok(());
        }

        // 2. Prepare data for efficient lookup
        let queued_soundeo_ids: std::collections::HashSet<String> =
            queued_tracks.iter().map(|t| t.track_id.clone()).collect();

        // 3. Prompt user to select playlists
        let mut local_playlists: Vec<_> = spotify_log.playlists.values().cloned().collect();
        local_playlists.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

        let playlist_names: Vec<String> = local_playlists.iter().map(|p| p.name.clone()).collect();

        let selections = Dialoguer::multiselect(
            "Select playlists to check against the queue (press space to select, enter to confirm)"
                .to_string(),
            playlist_names,
            Some(&vec![false; local_playlists.len()]),
            false,
        )
        .change_context(SpotifyError)?;

        if selections.is_empty() {
            println!("No playlists selected. Operation cancelled.");
            return Ok(());
        }

        println!("\n--- Queued Tracks Report ---");

        // 4. Process each selected playlist and print the count
        for index in selections {
            let selected_playlist = &local_playlists[index];
            let count = selected_playlist
                .tracks
                .keys()
                .filter_map(|spotify_id| spotify_log.soundeo_track_ids.get(spotify_id))
                .filter_map(|soundeo_id_opt| soundeo_id_opt.as_ref())
                .filter(|soundeo_id| queued_soundeo_ids.contains(*soundeo_id))
                .count();

            println!(
                "Playlist '{}': {} of {} tracks are in the queue.",
                selected_playlist.name.cyan(),
                count.to_string().green(),
                selected_playlist.tracks.len()
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

    async fn perform_spotify_login(user: &mut User) -> SpotifyResult<()> {
        // --- PKCE Step 1: Create a Code Verifier and Code Challenge ---
        let mut verifier_bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut verifier_bytes);
        let code_verifier =
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&verifier_bytes);

        let mut hasher = Sha256::new();
        hasher.update(code_verifier.as_bytes());
        let challenge_bytes = hasher.finalize();
        let code_challenge =
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(challenge_bytes);

        // --- Standard Auth Flow Steps ---
        let client_id = AppConfig::SPOTIFY_CLIENT_ID.to_string();
        let redirect_uri = "http://localhost:8888/callback";
        let scopes = "playlist-read-private playlist-read-collaborative";

        // 2. Start a temporary local server to catch the redirect
        let server = Server::http("127.0.0.1:8888").unwrap();

        // 3. Construct the authorization URL and open it in the browser
        let auth_url = format!(
            "https://accounts.spotify.com/authorize?response_type=code&client_id={}&scope={}&redirect_uri={}&code_challenge_method=S256&code_challenge={}",
            client_id,
            scopes.replace(' ', "%20"),
            redirect_uri,
            code_challenge
        );

        println!(
            "\n{}\n",
            "Please log in to Spotify in the browser window that just opened.".yellow()
        );
        if webbrowser::open(&auth_url).is_err() {
            println!(
                "Could not automatically open browser. Please copy/paste this URL:\n{}",
                auth_url.cyan()
            );
        }

        // 4. Wait for the user to log in and for Spotify to redirect back to our server
        let request = server.recv().into_report().change_context(SpotifyError)?;
        let full_url = format!("http://localhost:8888{}", request.url());
        let parsed_url = Url::parse(&full_url)
            .into_report()
            .change_context(SpotifyError)?;
        let auth_code = parsed_url
            .query_pairs()
            .find_map(|(key, value)| {
                if key == "code" {
                    Some(value.into_owned())
                } else {
                    None
                }
            })
            .ok_or(
                Report::new(SpotifyError).attach_printable("Could not find 'code' in callback URL"),
            )?;

        let response = Response::from_string(
            "<h1>Authentication successful!</h1><p>You can close this browser tab now.</p>",
        );
        request
            .respond(response)
            .into_report()
            .change_context(SpotifyError)?;
        println!("\nAuthorization code received successfully!");

        // 5. Exchange the code for a token
        let client = reqwest::Client::new();
        let params = [
            ("grant_type", "authorization_code".to_string()),
            ("code", auth_code),
            ("redirect_uri", redirect_uri.to_string()),
            ("client_id", client_id),
            ("code_verifier", code_verifier),
        ];

        let token_response: serde_json::Value = client
            .post("https://accounts.spotify.com/api/token")
            .form(&params)
            .send()
            .await
            .into_report()
            .change_context(SpotifyError)?
            .json()
            .await
            .into_report()
            .change_context(SpotifyError)?;

        // 6. Store the credentials in the user config
        user.spotify_access_token = token_response["access_token"]
            .as_str()
            .unwrap_or("")
            .to_string();
        user.spotify_refresh_token = token_response["refresh_token"]
            .as_str()
            .unwrap_or("")
            .to_string();

        if user.spotify_access_token.is_empty() {
            return Err(Report::new(SpotifyError).attach_printable(format!(
                "Failed to get access token. Response: {:?}",
                token_response
            )));
        }

        user.save_config_file().change_context(SpotifyError)?;

        println!(
            "{}",
            "Spotify login successful! Your credentials have been saved.".green()
        );

        Ok(())
    }
}
