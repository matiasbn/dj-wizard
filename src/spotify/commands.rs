use base64::Engine;
use clap::{Parser, Subcommand};
use colored::Colorize;
use error_stack::{IntoReport, Report, ResultExt};
use inflector::Inflector;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use strum::IntoEnumIterator;
use tiny_http::{Response, Server};
use url::Url;
use walkdir::WalkDir;
use webbrowser;

use crate::config::AppConfig;
use crate::dialoguer::Dialoguer;
use crate::log::DjWizardLog;
use crate::log::Priority;
use crate::queue::commands::QueueCommands;
use crate::soundeo::track::SoundeoTrack;
use crate::spotify::playlist::SpotifyPlaylist;
use crate::spotify::track::AutoPairResult;
use crate::spotify::track::SpotifyTrack;
use crate::spotify::SpotifyCRUD;
use crate::spotify::SpotifyError;
use crate::spotify::SpotifyResult;
use crate::user::{SoundeoUser, User};
use crate::Suggestion;

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

#[derive(Parser, Debug, Clone, PartialEq)]
pub struct SpotifyCli {
    #[command(subcommand)]
    pub command: Option<SpotifyCommands>,
}

#[derive(
    Debug,
    Deserialize,
    Serialize,
    Clone,
    strum_macros::Display,
    strum_macros::EnumIter,
    Subcommand,
    PartialEq,
)]
pub enum SpotifyCommands {
    /// Sync (update) all locally stored playlists with Spotify.
    SyncAllMyPlaylists,
    /// Download tracks from all playlists by automatically pairing single matches.
    DownloadFromAllPlaylists,
    /// Manually review and pair unpaired tracks from a specific playlist.
    ManuallyPairSpotifyWithSoundeoTracks,
    /// Download tracks from one or more playlists by pairing them with Soundeo.
    DownloadFromMultiplePlaylists,
    /// Organize downloaded tracks into folders named after their playlists.
    OrganizeDownloadsByPlaylist,
    /// Get a comprehensive status report for all playlists.
    GetPlaylistsStatus,
    /// Fetch your public Spotify playlists with the local log.
    FetchMyPublicPlaylists,
    /// Add a new Spotify playlist to track by providing its URL.
    AddNewPlaylistFromUrl,
    /// Remove one or more playlists from the local log.
    DeletePlaylists,
    // /// Refresh the track list and metadata for an existing local playlist.
    // UpdatePlaylistData,
    // /// Add all paired tracks from a playlist to the download queue.
    // QueueTracksFromPlaylist,
    // /// Count how many tracks from a playlist are currently in the download queue.
    // CountQueuedTracksByPlaylist,
    // /// Show a list of all downloaded tracks for a specific playlist.
    // PrintDownloadedTracksByPlaylist,
}

impl SpotifyCommands {
    pub async fn execute(cli: Option<SpotifyCli>) -> SpotifyResult<()> {
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

        // --- Auto-queue any paired tracks that are not yet in the queue ---
        println!("\nChecking for paired tracks that need to be queued...");
        let spotify_log = DjWizardLog::get_spotify().change_context(SpotifyError)?;
        let soundeo_log = DjWizardLog::get_soundeo().change_context(SpotifyError)?;
        let queued_tracks = DjWizardLog::get_queued_tracks().change_context(SpotifyError)?;

        let queued_ids: HashSet<String> =
            queued_tracks.iter().map(|t| t.track_id.clone()).collect();

        let tracks_to_enqueue: Vec<String> = spotify_log
            .soundeo_track_ids
            .iter()
            .filter_map(|(_, soundeo_id_option)| soundeo_id_option.as_ref())
            .filter(|soundeo_id| {
                // Condition: Not in queue AND not already downloaded
                !queued_ids.contains(*soundeo_id)
                    && soundeo_log
                        .tracks_info
                        .get(*soundeo_id)
                        .map_or(true, |info| !info.already_downloaded)
            })
            .cloned()
            .collect();

        if !tracks_to_enqueue.is_empty() {
            println!(
                "Found {} paired tracks to add to the High priority queue.",
                tracks_to_enqueue.len().to_string().green()
            );
            for soundeo_id in tracks_to_enqueue {
                DjWizardLog::add_queued_track(soundeo_id, Priority::High)
                    .change_context(SpotifyError)?;
            }
            println!("{}", "Auto-queueing complete.".green());
        } else {
            println!("No new paired tracks to queue.");
        }
        // --- End of auto-queue logic ---

        let command_to_run = match cli.and_then(|c| c.command) {
            Some(command) => command,
            None => {
                // No subcommand given, show interactive menu
                let options = Self::get_options();
                let selection = Dialoguer::select("Select".to_string(), options, None)
                    .change_context(SpotifyError)?;
                Self::get_selection(selection)
            }
        };

        return match command_to_run {
            SpotifyCommands::SyncAllMyPlaylists => {
                Self::sync_all_my_playlists(&mut user_config).await
            }
            SpotifyCommands::GetPlaylistsStatus => Self::get_playlists_status(),
            SpotifyCommands::DownloadFromAllPlaylists => Self::download_from_all_playlists().await,
            SpotifyCommands::AddNewPlaylistFromUrl => {
                Self::add_new_playlist(&mut user_config).await
            }
            // SpotifyCommands::UpdatePlaylistData => Self::update_playlist(&mut user_config).await,
            SpotifyCommands::FetchMyPublicPlaylists => {
                Self::sync_public_playlists(&mut user_config).await
            }
            SpotifyCommands::ManuallyPairTracksSpotifyWithSoundeoTracks => {
                Self::pair_and_queue_unpaired_tracks().await
            }
            // SpotifyCommands::QueueTracksFromPlaylist => Self::queue_tracks_from_playlist().await,
            // SpotifyCommands::PrintDownloadedTracksByPlaylist => {
            //     Self::print_downloaded_songs_by_playlist()
            // }
            SpotifyCommands::DownloadFromMultiplePlaylists => {
                Self::download_from_multiple_playlists().await
            }
            SpotifyCommands::DeletePlaylists => Self::delete_playlists(),
            // SpotifyCommands::CountQueuedTracksByPlaylist => Self::count_queued_tracks_by_playlist(),
            SpotifyCommands::OrganizeDownloadsByPlaylist => {
                Self::organize_downloads_by_playlist().await
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
                        "Update the playlist by running '{}' and select the update option",
                        "dj-wizard spotify".yellow()
                    ))));
            }
            None => {
                playlist
                    .get_playlist_info(user_config, true)
                    .await
                    .change_context(SpotifyError)?;
                DjWizardLog::update_spotify_playlist(playlist.clone())
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
        DjWizardLog::update_spotify_playlist(playlist.clone()).change_context(SpotifyError)?;
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

            DjWizardLog::update_spotify_playlist(playlist.clone()).change_context(SpotifyError)?;
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

    async fn sync_all_my_playlists(user_config: &mut User) -> SpotifyResult<()> {
        println!("\nStarting to sync all locally stored playlists...");

        let spotify_log = DjWizardLog::get_spotify().change_context(SpotifyError)?;
        if spotify_log.playlists.is_empty() {
            println!("{}", "No playlists to sync. Add some first!".yellow());
            return Ok(());
        }

        let playlists_to_sync: Vec<SpotifyPlaylist> =
            spotify_log.playlists.values().cloned().collect();
        let total_playlists = playlists_to_sync.len();

        println!("Found {} playlists to sync.", total_playlists);

        for (i, mut playlist) in playlists_to_sync.into_iter().enumerate() {
            println!(
                "\n({}/{}) Syncing playlist: {}",
                i + 1,
                total_playlists,
                playlist.name.clone().cyan()
            );

            // The get_playlist_info function modifies the playlist in place
            match playlist.get_playlist_info(user_config, false).await {
                Ok(_) => {
                    // Save the updated playlist back to the log
                    DjWizardLog::update_spotify_playlist(playlist.clone())
                        .change_context(SpotifyError)?;
                    println!(
                        "  └─ {} Successfully synced '{}'.",
                        "✔".green(),
                        playlist.name.green()
                    );
                }
                Err(e) => {
                    println!(
                        "  └─ {} Failed to sync '{}'. Error: {:?}",
                        "✖".red(),
                        playlist.name.red(),
                        e
                    );
                }
            }
        }

        println!("\n{}", "All playlists have been processed.".green());
        Ok(())
    }

    async fn pair_and_queue_unpaired_tracks() -> SpotifyResult<()> {
        let mut playlist = SpotifyPlaylist::prompt_select_playlist(
            "Select a playlist to manually pair tracks from",
        )?;
        let mut soundeo_user = SoundeoUser::new().change_context(SpotifyError)?;
        soundeo_user
            .login_and_update_user_info()
            .await
            .change_context(SpotifyError)?;

        let spotify_log = DjWizardLog::get_spotify().change_context(SpotifyError)?;

        let unpaired_tracks: Vec<_> = playlist
            .tracks
            .iter()
            .filter(|(id, _)| !spotify_log.soundeo_track_ids.contains_key(*id))
            .map(|(id, track)| (id.clone(), track.clone()))
            .collect();

        if unpaired_tracks.is_empty() {
            println!("\nNo unpaired tracks found in '{}'.", playlist.name.cyan());
            return Ok(());
        }

        println!(
            "Found {} unpaired tracks in '{}'.",
            unpaired_tracks.len().to_string().cyan(),
            playlist.name.yellow()
        );

        // --- Phase 1: Automatic Pairing Pass ---
        println!("\n--- Phase 1: Automatic Pairing Pass ---");
        let mut paired_count = 0;
        let mut needs_manual_review: Vec<(String, SpotifyTrack)> = Vec::new();

        let unpaired_len = unpaired_tracks.len();
        for (i, (spotify_track_id, mut spotify_track)) in unpaired_tracks.into_iter().enumerate() {
            println!(
                "Processing track {}/{}: {} by {}",
                format!("{}", i + 1).cyan(),
                format!("{}", unpaired_len).cyan(),
                spotify_track.title.yellow(),
                spotify_track.artists.yellow()
            );
            let result = spotify_track.find_single_soundeo_match(&soundeo_user).await;

            match result {
                Ok(pair_result) => match pair_result {
                    AutoPairResult::Paired(soundeo_id) => {
                        paired_count += 1;
                        println!("  └─ {} Paired automatically.", "✔".green());
                        DjWizardLog::update_spotify_to_soundeo_track(
                            spotify_track_id.clone(),
                            Some(soundeo_id.clone()),
                        )
                        .change_context(SpotifyError)?;

                        DjWizardLog::add_queued_track(soundeo_id, Priority::High)
                            .change_context(SpotifyError)?;
                    }
                    AutoPairResult::NoMatch | AutoPairResult::MultipleMatches(_) => {
                        println!("  └─ {} Needs manual review.", "…".yellow());
                        needs_manual_review.push((spotify_track_id, spotify_track));
                    }
                },
                Err(_) => {
                    println!(
                        "  └─ {} Error during search. Needs manual review.",
                        "✖".red()
                    );
                    needs_manual_review.push((spotify_track_id, spotify_track));
                }
            }
        }

        println!(
            "\nAutomatic pass complete. Paired and queued {} tracks.",
            paired_count.to_string().green()
        );

        // --- Phase 2: Manual Pairing Pass ---
        if needs_manual_review.is_empty() {
            println!("{}", "No tracks require manual review.".green());
            return Ok(());
        }

        println!("\n--- Phase 2: Manual Pairing Pass ---");
        println!(
            "There are {} tracks that need manual review. Starting interactive session...",
            needs_manual_review.len().to_string().yellow()
        );

        playlist
            .pair_unpaired_tracks(&mut soundeo_user)
            .await
            .change_context(SpotifyError)?;

        Ok(())
    }

    async fn queue_tracks_from_playlist() -> SpotifyResult<()> {
        let playlist =
            SpotifyPlaylist::prompt_select_playlist("Select the playlist to queue tracks from")?;

        let spotify_log = DjWizardLog::get_spotify().change_context(SpotifyError)?;
        let soundeo_log = DjWizardLog::get_soundeo().change_context(SpotifyError)?;

        let all_paired_ids: Vec<String> = playlist
            .tracks
            .keys()
            .filter_map(|spotify_id| spotify_log.soundeo_track_ids.get(spotify_id))
            .filter_map(|soundeo_id_option| soundeo_id_option.as_ref())
            .cloned()
            .collect();

        if all_paired_ids.is_empty() {
            println!(
                "{}",
                "No paired tracks found for this playlist. Please pair them first.".yellow()
            );
            return Ok(());
        }

        // Filter out tracks that are already downloaded
        let soundeo_ids_to_queue: Vec<String> = all_paired_ids
            .iter()
            .filter(|soundeo_id| {
                soundeo_log
                    .tracks_info
                    .get(*soundeo_id)
                    .map_or(true, |track_info| !track_info.already_downloaded)
            })
            .cloned()
            .collect();

        let already_downloaded_count = all_paired_ids.len() - soundeo_ids_to_queue.len();

        if soundeo_ids_to_queue.is_empty() {
            println!(
                "{}",
                format!(
                    "All {} paired tracks in this playlist have already been downloaded.",
                    all_paired_ids.len()
                )
                .yellow()
            );
            return Ok(());
        }

        println!(
            "Found {} paired tracks in playlist '{}'. {} of them are not yet downloaded.",
            all_paired_ids.len(),
            playlist.name.cyan(),
            soundeo_ids_to_queue.len().to_string().green()
        );
        if already_downloaded_count > 0 {
            println!(
                "Skipping {} tracks that have already been downloaded.",
                already_downloaded_count.to_string().yellow()
            );
        }

        let mut queued_count = 0;
        let mut skipped_count = 0;
        for soundeo_id in soundeo_ids_to_queue {
            if DjWizardLog::add_queued_track(soundeo_id, Priority::High)
                .change_context(SpotifyError)?
            {
                queued_count += 1;
            } else {
                skipped_count += 1;
            }
        }

        if queued_count > 0 {
            println!(
                "Successfully queued {} new tracks with {} priority.",
                queued_count.to_string().green(),
                format!("{:?}", Priority::High).cyan()
            );
        }
        if skipped_count > 0 {
            println!(
                "Skipped {} tracks that were already in the queue.",
                skipped_count.to_string().yellow()
            );
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

        println!("\n--- Queued Tracks Report ---");

        let queued_soundeo_ids: std::collections::HashSet<String> =
            queued_tracks.iter().map(|t| t.track_id.clone()).collect();

        let mut local_playlists: Vec<_> = spotify_log.playlists.values().cloned().collect();
        local_playlists.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

        // Process each playlist and print the count
        for playlist in local_playlists {
            let mut count = 0;
            // Iterate over each track in the selected Spotify playlist
            for spotify_track_id in playlist.tracks.keys() {
                // Check if this Spotify track has a Soundeo track paired with it
                if let Some(Some(soundeo_track_id)) =
                    spotify_log.soundeo_track_ids.get(spotify_track_id)
                {
                    // If it's paired, check if that Soundeo track ID is in our set of queued tracks
                    if queued_soundeo_ids.contains(soundeo_track_id) {
                        count += 1;
                    }
                }
            }

            println!(
                "Playlist '{}': {} of {} tracks are in the queue.",
                playlist.name.cyan(),
                count.to_string().green(),
                playlist.tracks.len()
            );
        }

        Ok(())
    }

    fn get_playlists_status() -> SpotifyResult<()> {
        println!("\n--- Playlists Status Report ---");

        // 1. Load all necessary data from the logs
        let spotify_log = DjWizardLog::get_spotify().change_context(SpotifyError)?;
        let soundeo_log = DjWizardLog::get_soundeo().change_context(SpotifyError)?;
        let queued_tracks = DjWizardLog::get_queued_tracks().change_context(SpotifyError)?;

        if spotify_log.playlists.is_empty() {
            println!("{}", "No playlists found in the log.".yellow());
            return Ok(());
        }

        // 2. Prepare lookups for efficient checking
        let queued_ids: HashSet<String> =
            queued_tracks.iter().map(|t| t.track_id.clone()).collect();

        let mut playlists: Vec<_> = spotify_log.playlists.values().cloned().collect();
        playlists.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

        let mut total_tracks_count = 0;
        let mut total_downloaded = 0;
        let mut total_queued = 0;
        let mut total_pending_pairing = 0;
        let mut total_no_match = 0;

        // 3. Iterate through each playlist and categorize its tracks
        for playlist in playlists {
            let mut downloaded = 0;
            let mut queued = 0;
            let mut pending_pairing = 0;
            let mut no_match = 0;

            for spotify_track in playlist.tracks.values() {
                match spotify_log
                    .soundeo_track_ids
                    .get(&spotify_track.spotify_track_id)
                {
                    None => {
                        // Not yet processed
                        pending_pairing += 1;
                    }
                    Some(None) => {
                        // Processed, but no single match found
                        no_match += 1;
                    }
                    Some(Some(soundeo_id)) => {
                        // Successfully paired, now check its status
                        if soundeo_log
                            .tracks_info
                            .get(soundeo_id)
                            .map_or(false, |info| info.already_downloaded)
                        {
                            downloaded += 1;
                        }
                        if queued_ids.contains(soundeo_id) {
                            queued += 1;
                        }
                    }
                }
            }

            total_tracks_count += playlist.tracks.len();
            total_downloaded += downloaded;
            total_queued += queued;
            total_pending_pairing += pending_pairing;
            total_no_match += no_match;

            // 4. Print the report for the current playlist
            println!("\nPlaylist: {}", playlist.name.bright_blue().bold());
            println!(
                "  - Total Tracks:      {}",
                playlist.tracks.len().to_string().white()
            );
            println!("  - Downloaded:        {}", downloaded.to_string().green());
            println!("  - In Queue:          {}", queued.to_string().cyan());
            println!(
                "  - Pending Pairing:   {}",
                pending_pairing.to_string().yellow()
            );
            println!("  - No Match Found:    {}", no_match.to_string().red());
        }

        // 5. Print the grand total summary
        println!("\n--- Grand Total Summary ---");
        println!(
            "  - Total Tracks:      {}",
            total_tracks_count.to_string().white()
        );
        println!(
            "  - Downloaded:        {}",
            total_downloaded.to_string().green()
        );
        println!("  - In Queue:          {}", total_queued.to_string().cyan());
        println!(
            "  - Pending Pairing:   {}",
            total_pending_pairing.to_string().yellow()
        );
        println!(
            "  - No Match Found:    {}",
            total_no_match.to_string().red()
        );

        Ok(())
    }

    async fn download_from_all_playlists() -> SpotifyResult<()> {
        // 1. Login to Soundeo
        println!("\nLogging into Soundeo to pair and queue tracks...");
        let mut soundeo_user = SoundeoUser::new().change_context(SpotifyError)?;
        soundeo_user
            .login_and_update_user_info()
            .await
            .change_context(SpotifyError)?;

        // 2. Get all playlists
        let spotify_log = DjWizardLog::get_spotify().change_context(SpotifyError)?;
        if spotify_log.playlists.is_empty() {
            println!(
                "{}",
                "No Spotify playlists found. Please sync or add a playlist first.".yellow()
            );
            return Ok(());
        }
        let all_playlists: Vec<SpotifyPlaylist> = spotify_log.playlists.values().cloned().collect();
        println!("Found {} playlists to process.", all_playlists.len());

        let mut newly_paired_soundeo_ids: Vec<String> = Vec::new();
        let mut any_tracks_failed_pairing = false;

        // 3. Iterate and auto-pair
        for playlist in all_playlists {
            println!("\nProcessing playlist: {}", playlist.name.yellow());

            let mut paired_in_playlist = 0;
            let mut skipped_in_playlist = 0;
            let mut errors_in_playlist = 0;

            // Refetch the log inside the loop to get the most recent pairings
            // from previous playlists in this same run.
            let current_spotify_log = DjWizardLog::get_spotify().change_context(SpotifyError)?;

            let unpaired_tracks: Vec<_> = playlist
                .tracks
                .values()
                .filter(|track| {
                    !current_spotify_log
                        .soundeo_track_ids
                        .contains_key(&track.spotify_track_id)
                })
                .cloned()
                .collect();

            if unpaired_tracks.is_empty() {
                println!("  └─ No unpaired tracks found in this playlist.");
                continue;
            }

            println!(
                "  Found {} unpaired tracks. Attempting to auto-pair...",
                unpaired_tracks.len()
            );

            let total_unpaired = unpaired_tracks.len();
            for (i, mut spotify_track) in unpaired_tracks.into_iter().enumerate() {
                let pairing_result = spotify_track.find_single_soundeo_match(&soundeo_user).await;

                match pairing_result {
                    Ok(result) => {
                        match result {
                            crate::spotify::track::AutoPairResult::Paired(soundeo_id) => {
                                println!(
                                    "    └─ ({}/{}) {} Paired: {} - {}",
                                    format!("{}", i + 1).cyan(),
                                    format!("{}", total_unpaired).cyan(),
                                    "✔".green(),
                                    spotify_track.artists.cyan(),
                                    spotify_track.title.cyan()
                                );
                                DjWizardLog::update_spotify_to_soundeo_track(
                                    spotify_track.spotify_track_id.clone(),
                                    Some(soundeo_id.clone()),
                                )
                                .change_context(SpotifyError)?;
                                newly_paired_soundeo_ids.push(soundeo_id);
                                paired_in_playlist += 1;
                            }
                            crate::spotify::track::AutoPairResult::NoMatch => {
                                println!(
                                    "    └─ ({}/{}) {} No match found for: {} - {}",
                                    format!("{}", i + 1).cyan(),
                                    format!("{}", total_unpaired).cyan(),
                                    "✖".yellow(),
                                    spotify_track.artists.cyan(),
                                    spotify_track.title.cyan()
                                );
                                DjWizardLog::update_spotify_to_soundeo_track(
                                    spotify_track.spotify_track_id.clone(),
                                    None,
                                )
                                .change_context(SpotifyError)?;
                                skipped_in_playlist += 1;
                                any_tracks_failed_pairing = true;
                            }
                            crate::spotify::track::AutoPairResult::MultipleMatches(results) => {
                                println!(
                                    "    └─ ({}/{}) {} Multiple matches for: {} - {}",
                                    format!("{}", i + 1).cyan(),
                                    format!("{}", total_unpaired).cyan(),
                                    "✖".yellow(),
                                    spotify_track.artists.cyan(),
                                    spotify_track.title.cyan()
                                );
                                DjWizardLog::update_spotify_to_soundeo_track(
                                    spotify_track.spotify_track_id.clone(),
                                    None,
                                )
                                .change_context(SpotifyError)?;
                                DjWizardLog::add_to_multiple_matches_cache(
                                    spotify_track.spotify_track_id.clone(),
                                    results,
                                )
                                .change_context(SpotifyError)?;
                                skipped_in_playlist += 1;
                                any_tracks_failed_pairing = true;
                            }
                        };
                    }
                    Err(_) => {
                        println!(
                            "    └─ ({}/{}) {} Error pairing track: {}",
                            format!("{}", i + 1).cyan(),
                            format!("{}", total_unpaired).cyan(),
                            "✖".red(),
                            spotify_track.title.cyan()
                        );
                        errors_in_playlist += 1;
                        any_tracks_failed_pairing = true;
                    }
                }
            }

            println!(
                "  └─ Playlist summary: {} Paired, {} Skipped, {} Errors.",
                paired_in_playlist.to_string().green(),
                skipped_in_playlist.to_string().yellow(),
                errors_in_playlist.to_string().red()
            );
        }

        // 4. Queue all newly paired tracks
        if newly_paired_soundeo_ids.is_empty() {
            println!("\nNo new tracks were successfully paired. Nothing to queue.");
            if any_tracks_failed_pairing {
                println!(
                    "{}",
                    "Some tracks could not be paired automatically. You can try pairing them using the 'Manually pair tracks' command.".yellow()
                );
            }
            return Ok(());
        }

        println!("\n--- Pairing Complete ---");
        println!(
            "Successfully auto-paired a total of {} new tracks.",
            newly_paired_soundeo_ids.len().to_string().green()
        );
        if any_tracks_failed_pairing {
            println!(
                "{}",
                "Some tracks could not be paired automatically. You can try pairing them using the 'Manually pair tracks' command.".yellow()
            );
        }

        println!("Adding newly paired tracks to the queue with High priority...");
        let mut queued_count = 0;
        for soundeo_id in newly_paired_soundeo_ids {
            if DjWizardLog::add_queued_track(soundeo_id, Priority::High)
                .change_context(SpotifyError)?
            {
                queued_count += 1;
            }
        }

        // 5. Start download
        if queued_count > 0 {
            println!(
                "\nSuccessfully queued {} new tracks.",
                queued_count.to_string().green()
            );
            println!("Starting download process automatically...");
            QueueCommands::resume_queue(true)
                .await
                .change_context(SpotifyError)
                .attach_printable("Failed to start the download queue.")?;
        } else {
            println!(
                "\nAll newly paired tracks were already in the queue. No new downloads started."
            );
        }

        Ok(())
    }

    async fn download_from_multiple_playlists() -> SpotifyResult<()> {
        // 1. Get playlists and let user select multiple
        let spotify_log = DjWizardLog::get_spotify().change_context(SpotifyError)?;
        if spotify_log.playlists.is_empty() {
            println!(
                "{}",
                "No Spotify playlists found. Please sync or add a playlist first.".yellow()
            );
            return Ok(());
        }

        let mut playlists: Vec<_> = spotify_log.playlists.values().cloned().collect();
        playlists.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

        let playlist_names: Vec<String> = playlists.iter().map(|p| p.name.clone()).collect();

        let selections = Dialoguer::multiselect(
            "Select playlists to build your queue from (space to select, enter to confirm)"
                .to_string(),
            playlist_names,
            Some(&vec![false; playlists.len()]),
            false,
        )
        .change_context(SpotifyError)?;

        if selections.is_empty() {
            println!("No playlists selected. Operation cancelled.");
            return Ok(());
        }

        let selected_playlists: Vec<SpotifyPlaylist> =
            selections.iter().map(|&i| playlists[i].clone()).collect();

        // 3. Set priority to High as per new global rule
        let selected_priority = Priority::High;
        // 4. Log into Soundeo and start processing
        println!("\nLogging into Soundeo to pair and queue tracks...");
        let mut soundeo_user = SoundeoUser::new().change_context(SpotifyError)?;
        soundeo_user
            .login_and_update_user_info()
            .await
            .change_context(SpotifyError)?;

        let mut total_queued = 0;
        let mut total_failed = 0;

        for playlist in selected_playlists {
            println!("\nProcessing playlist: {}", playlist.name.yellow());

            let current_spotify_log = DjWizardLog::get_spotify().change_context(SpotifyError)?;

            let all_unpaired_tracks: Vec<_> = playlist
                .tracks
                .values()
                .filter(|track| {
                    !current_spotify_log
                        .soundeo_track_ids
                        .contains_key(&track.spotify_track_id)
                })
                .cloned()
                .collect();

            if all_unpaired_tracks.is_empty() {
                println!("  └─ No unpaired tracks found to process in this playlist. Skipping.");
                continue;
            }

            println!(
                "  Attempting to pair and queue all {} unpaired tracks...",
                all_unpaired_tracks.len().to_string().cyan()
            );

            let mut processed_count = 0;
            let all_unpaired_len = all_unpaired_tracks.len();

            for mut spotify_track in all_unpaired_tracks {
                processed_count += 1;
                println!(
                    "    ({}/{}) Processing: {} by {}",
                    processed_count.to_string().cyan(),
                    all_unpaired_len.to_string().cyan(),
                    spotify_track.title.cyan(),
                    spotify_track.artists.cyan()
                );

                let pairing_result = spotify_track.find_single_soundeo_match(&soundeo_user).await;

                match pairing_result {
                    Ok(result) => {
                        match result {
                            crate::spotify::track::AutoPairResult::Paired(soundeo_id) => {
                                println!("      └─ {} Paired automatically.", "✔".green());
                                DjWizardLog::update_spotify_to_soundeo_track(
                                    spotify_track.spotify_track_id.clone(),
                                    Some(soundeo_id.clone()),
                                )
                                .change_context(SpotifyError)?;

                                if DjWizardLog::add_queued_track(soundeo_id, selected_priority)
                                    .change_context(SpotifyError)?
                                {
                                    println!(
                                        "        └─ Added to download queue with {} priority.",
                                        format!("{:?}", selected_priority).cyan()
                                    );
                                    total_queued += 1;
                                } else {
                                    println!("        └─ Already in download queue.");
                                }
                            }
                            _ => {
                                println!(
                                    "      └─ {} Not auto-paired (multiple matches or no match found).",
                                    "✖".red()
                                );
                                // Mark as processed so we don't check it again in this flow
                                DjWizardLog::update_spotify_to_soundeo_track(
                                    spotify_track.spotify_track_id.clone(),
                                    None,
                                )
                                .change_context(SpotifyError)?;
                                total_failed += 1;
                            }
                        }
                    }
                    Err(_) => {
                        println!(
                            "      └─ {} Error pairing track: {}. Skipping.",
                            "✖".red(),
                            spotify_track.title.cyan()
                        );
                        total_failed += 1;
                    }
                }
            }
        }

        // 5. Summary and start download
        println!("\n--- Curation Complete ---");
        println!(
            "Successfully queued {} new tracks.",
            total_queued.to_string().green()
        );
        if total_failed > 0 {
            println!(
                "Failed to automatically pair {} tracks.",
                total_failed.to_string().yellow()
            );
            println!("You can try pairing them manually later.");
        }

        if total_queued > 0 {
            println!("\nStarting download process automatically...");
            QueueCommands::resume_queue(true)
                .await
                .change_context(SpotifyError)
                .attach_printable("Failed to start the download queue.")?;
        }

        Ok(())
    }

    fn create_spotify_playlist_file() -> SpotifyResult<()> {
        let prompt_text = "Select the playlist to create the m3u8 file";
        let _playlist = SpotifyPlaylist::prompt_select_playlist(prompt_text)?;
        let _file_content = "#EXTM3U";

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

    async fn organize_downloads_by_playlist() -> SpotifyResult<()> {
        // Phase 1: Preparation and Selection
        let mut user_config = User::new();
        user_config
            .read_config_file()
            .change_context(SpotifyError)?;
        let download_dir = PathBuf::from(&user_config.download_path);
        if !download_dir.exists() {
            println!(
                "{}",
                "Download directory not found. Nothing to organize.".yellow()
            );
            return Ok(());
        }

        let spotify_log = DjWizardLog::get_spotify().change_context(SpotifyError)?;
        if spotify_log.playlists.is_empty() {
            println!(
                "{}",
                "No Spotify playlists found. Please sync or add a playlist first.".yellow()
            );
            return Ok(());
        }

        let mut local_playlists: Vec<_> = spotify_log.playlists.values().cloned().collect();
        local_playlists.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

        let playlist_names: Vec<String> = local_playlists.iter().map(|p| p.name.clone()).collect();
        let defaults = vec![true; playlist_names.len()];

        let selections = Dialoguer::multiselect(
            "Select playlists to organize (all are selected by default, space to deselect)"
                .to_string(),
            playlist_names,
            Some(&defaults),
            false,
        )
        .change_context(SpotifyError)?;

        if selections.is_empty() {
            println!("No playlists selected. Operation cancelled.");
            return Ok(());
        }

        let selected_playlists: Vec<SpotifyPlaylist> = selections
            .iter()
            .map(|&i| local_playlists[i].clone())
            .collect();

        // Scan local files recursively to build a master index
        println!("\nScanning local download directory (recursively)...");
        let mut local_files: HashMap<String, PathBuf> = HashMap::new();
        for entry in WalkDir::new(&download_dir)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if path.is_file() {
                if let Some(extension) = path.extension() {
                    if extension == "AIFF" || extension == "aiff" {
                        if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
                            local_files.insert(file_name.to_string(), path.to_path_buf());
                        }
                    }
                }
            }
        }
        println!("Found {} local .AIFF files in total.", local_files.len());

        // Phase 2: Processing and Classification
        let soundeo_log = DjWizardLog::get_soundeo().change_context(SpotifyError)?;
        let mut tracks_to_force_redownload: Vec<SoundeoTrack> = Vec::new();
        let mut tracks_to_report_missing: HashMap<String, Vec<String>> = HashMap::new();

        println!("\nAnalyzing playlists and organizing files...");

        for playlist in selected_playlists {
            let sanitized_playlist_name = Self::sanitize_filename(&playlist.name);
            let playlist_folder_path = download_dir.join(&sanitized_playlist_name);
            fs::create_dir_all(&playlist_folder_path)
                .into_report()
                .change_context(SpotifyError)?;

            let mut present_songs_count = 0;

            for spotify_track in playlist.tracks.values() {
                if let Some(Some(soundeo_id)) = spotify_log
                    .soundeo_track_ids
                    .get(&spotify_track.spotify_track_id)
                {
                    if let Some(soundeo_track) = soundeo_log.tracks_info.get(soundeo_id) {
                        let expected_filename = format!("{}.AIFF", soundeo_track.title);

                        if let Some(source_path) = local_files.get(&expected_filename) {
                            present_songs_count += 1;
                            let dest_path = playlist_folder_path.join(&expected_filename);
                            if !dest_path.exists() {
                                fs::copy(source_path, &dest_path)
                                    .into_report()
                                    .change_context(SpotifyError)?;
                            }
                        } else {
                            // The file is genuinely missing from the disk.
                            if soundeo_track.already_downloaded {
                                // It was downloaded before, so we can re-download it.
                                // Avoid duplicates in the redownload list.
                                if !tracks_to_force_redownload
                                    .iter()
                                    .any(|t| &t.id == soundeo_id)
                                {
                                    tracks_to_force_redownload.push(soundeo_track.clone());
                                }
                            } else {
                                // It was never downloaded, or not paired properly. Report it.
                                tracks_to_report_missing
                                    .entry(playlist.name.clone())
                                    .or_default()
                                    .push(format!(
                                        "{} - {}",
                                        spotify_track.artists, spotify_track.title
                                    ));
                            }
                        }
                    }
                } else {
                    tracks_to_report_missing
                        .entry(playlist.name.clone())
                        .or_default()
                        .push(format!(
                            "{} - {}",
                            spotify_track.artists, spotify_track.title
                        ));
                }
            }

            println!(
                "{} of {} songs from playlist '{}' are available in the local playlist folder.",
                present_songs_count.to_string().green(),
                playlist.tracks.len(),
                playlist.name.cyan()
            );
        }

        // Phase 3: Actions and Reporting
        println!("\n--- Organization Complete ---");

        if tracks_to_report_missing.len() > 1 {
            println!(
                "\n{}",
                "The following tracks need to be paired or downloaded for the first time.".yellow()
            );
            println!(
                "Please use the '{}' or '{}' command.",
                "Manually pair tracks".cyan(),
                "Download from multiple playlists".cyan()
            );
            for (playlist_name, track_titles) in tracks_to_report_missing {
                println!("\nPlaylist '{}':", playlist_name.yellow());
                for title in track_titles {
                    println!("- {}", title);
                }
            }
        }

        if !tracks_to_force_redownload.is_empty() {
            println!(
                "\nFound {} tracks that were previously downloaded but are missing locally.",
                tracks_to_force_redownload.len().to_string().green()
            );
            println!("Starting re-download process automatically...");

            let mut soundeo_user = SoundeoUser::new().change_context(SpotifyError)?;
            soundeo_user
                .login_and_update_user_info()
                .await
                .change_context(SpotifyError)?;

            for mut track in tracks_to_force_redownload {
                // The `true` flag forces the re-download, ignoring the `already_downloaded` state.
                if let Err(e) = track.download_track(&mut soundeo_user, true, true).await {
                    println!(
                        "Failed to re-download track '{}': {:?}",
                        track.title.red(),
                        e
                    );
                }
            }
            println!("\n{}", "Re-download process finished.".green());
        }

        Ok(())
    }

    fn sanitize_filename(name: &str) -> String {
        name.chars()
            .filter(|c| !matches!(*c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
            .collect()
    }
}
