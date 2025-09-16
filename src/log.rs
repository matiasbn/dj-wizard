use std::collections::{HashMap, HashSet};
use std::fs::read_to_string;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use std::{fmt, fs};

use crate::artist::{ArtistCRUD, ArtistManager};
use crate::auth::{firebase_client::FirebaseClient, google_auth::GoogleAuth};
use crate::url_list::UrlListCRUD;
use colored::Colorize;
use error_stack::{IntoReport, Report, ResultExt};
use reqwest::blocking::multipart::Form;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::genre_tracker::{GenreTracker, GenreTrackerCRUD};
use crate::soundeo::search_bar::SoundeoSearchBarResult;
use crate::soundeo::track::SoundeoTrack;
use crate::soundeo::{Soundeo, SoundeoCRUD};
use crate::spotify::playlist::SpotifyPlaylist;
use crate::spotify::{Spotify, SpotifyCRUD};
use crate::user::{IPFSConfig, SoundeoUser, User};

#[derive(Debug)]
pub struct DjWizardLogError;
impl fmt::Display for DjWizardLogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Dj Wizard log error")
    }
}
impl std::error::Error for DjWizardLogError {}

pub type DjWizardLogResult<T> = error_stack::Result<T, DjWizardLogError>;

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    High,
    Normal,
    Low,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct QueuedTrack {
    pub track_id: String,
    pub priority: Priority,
    pub order_key: f64,
    pub added_at: u64,
    #[serde(default)]
    pub migrated: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DjWizardLog {
    pub last_update: u64,
    pub queued_tracks: Vec<QueuedTrack>,
    #[serde(default)]
    pub available_tracks: HashSet<String>,
    #[serde(default)]
    pub url_list: HashSet<String>,
    pub spotify: Spotify,
    pub soundeo: Soundeo,
    #[serde(default)]
    pub genre_tracker: GenreTracker,
    #[serde(default)]
    pub artist_manager: ArtistManager,
    /// Track IDs that already exist in Firebase (for bulk filtering)
    #[serde(default)]
    pub firebase_migrated_tracks: Option<Vec<String>>,
    /// Queue IDs that already exist in Firebase (for bulk filtering)
    #[serde(default)]
    pub firebase_migrated_queues: Option<Vec<String>>,
}

impl DjWizardLog {
    pub fn get_queued_tracks() -> DjWizardLogResult<Vec<QueuedTrack>> {
        // Use Firebase only - no fallback to local
        Ok(tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                // Load auth token
                let auth_token = GoogleAuth::load_token().await.map_err(|_| "No auth")?;

                // Create Firebase client
                let firebase_client = FirebaseClient::new(auth_token)
                    .await
                    .map_err(|_| "Firebase unavailable")?;

                // Get queued tracks from Firebase
                firebase_client
                    .get_queued_tracks()
                    .await
                    .map_err(|_| "Failed to get queued tracks from Firebase")
            })
        })
        .map_err(|_: &str| error_stack::Report::new(DjWizardLogError))?)
    }

    pub fn get_available_tracks() -> DjWizardLogResult<HashSet<String>> {
        // Use Firebase only - no fallback to local
        Ok(tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                // Load auth token
                let auth_token = GoogleAuth::load_token().await.map_err(|_| "No auth")?;

                // Create Firebase client
                let firebase_client = FirebaseClient::new(auth_token)
                    .await
                    .map_err(|_| "Firebase unavailable")?;

                // Get available tracks from Firebase
                firebase_client
                    .get_available_tracks()
                    .await
                    .map_err(|_| "Failed to get available tracks from Firebase")
            })
        })
        .map_err(|_: &str| error_stack::Report::new(DjWizardLogError))?)
    }

    pub fn get_spotify() -> DjWizardLogResult<Spotify> {
        // Use Firebase only - no fallback to local
        Ok(tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let auth_token = GoogleAuth::load_token().await.map_err(|_| "No auth")?;
                let firebase_client = FirebaseClient::new(auth_token)
                    .await
                    .map_err(|_| "Firebase unavailable")?;
                firebase_client
                    .get_spotify()
                    .await
                    .map_err(|_| "Failed to get spotify from Firebase")
            })
        })
        .map_err(|_: &str| error_stack::Report::new(DjWizardLogError))?)
    }

    pub fn get_soundeo() -> DjWizardLogResult<Soundeo> {
        // Use Firebase only - no fallback to local
        Ok(tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let auth_token = GoogleAuth::load_token().await.map_err(|_| "No auth")?;
                let firebase_client = FirebaseClient::new(auth_token)
                    .await
                    .map_err(|_| "Firebase unavailable")?;
                firebase_client
                    .get_soundeo()
                    .await
                    .map_err(|_| "Failed to get soundeo from Firebase")
            })
        })
        .map_err(|_: &str| error_stack::Report::new(DjWizardLogError))?)
    }

    pub fn get_soundeo_tracks_info() -> DjWizardLogResult<std::collections::HashMap<String, crate::soundeo::track::SoundeoTrack>> {
        // Use Firebase only - optimized method that returns just the HashMap
        Ok(tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let auth_token = GoogleAuth::load_token().await.map_err(|_| "No auth")?;
                let firebase_client = FirebaseClient::new(auth_token).await.map_err(|_| "Firebase unavailable")?;
                firebase_client.get_soundeo_tracks_info().await.map_err(|_| "Failed to get soundeo tracks info from Firebase")
            })
        }).map_err(|_: &str| error_stack::Report::new(DjWizardLogError))?)
    }

    pub fn get_url_list() -> DjWizardLogResult<HashSet<String>> {
        // Use Firebase only - no fallback to local
        Ok(tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let auth_token = GoogleAuth::load_token().await.map_err(|_| "No auth")?;
                let firebase_client = FirebaseClient::new(auth_token)
                    .await
                    .map_err(|_| "Firebase unavailable")?;
                firebase_client
                    .get_url_list()
                    .await
                    .map_err(|_| "Failed to get url_list from Firebase")
            })
        })
        .map_err(|_: &str| error_stack::Report::new(DjWizardLogError))?)
    }

    /// Mark a track as migrated to Firebase
    pub fn mark_track_as_migrated(track_id: &str) -> DjWizardLogResult<()> {
        let mut log = Self::read_log()?;

        if let Some(track) = log.soundeo.tracks_info.get_mut(track_id) {
            track.migrated = true;
            log.save_log()?;
        }

        Ok(())
    }

    /// Set all Firebase migrated track IDs in bulk (much faster than individual marking)
    pub fn set_firebase_migrated_tracks(track_ids: Vec<String>) -> DjWizardLogResult<()> {
        let mut log = Self::read_log()?;

        // Add new field to store Firebase migrated track IDs
        log.firebase_migrated_tracks = Some(track_ids);
        log.save_log()?;

        Ok(())
    }

    /// Get Firebase migrated track IDs
    pub fn get_firebase_migrated_tracks() -> DjWizardLogResult<Vec<String>> {
        let log = Self::read_log()?;
        Ok(log.firebase_migrated_tracks.unwrap_or_default())
    }

    /// Set all Firebase migrated queue IDs in bulk (much faster than individual marking)
    pub fn set_firebase_migrated_queues(queue_ids: Vec<String>) -> DjWizardLogResult<()> {
        let mut log = Self::read_log()?;

        // Add new field to store Firebase migrated queue IDs
        log.firebase_migrated_queues = Some(queue_ids);
        log.save_log()?;

        Ok(())
    }

    /// Get Firebase migrated queue IDs
    pub fn get_firebase_migrated_queues() -> DjWizardLogResult<Vec<String>> {
        let log = Self::read_log()?;
        Ok(log.firebase_migrated_queues.unwrap_or_default())
    }

    pub fn get_genre_tracker() -> DjWizardLogResult<GenreTracker> {
        // Use Firebase only - no fallback to local
        Ok(tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let auth_token = GoogleAuth::load_token().await.map_err(|_| "No auth")?;
                let firebase_client = FirebaseClient::new(auth_token)
                    .await
                    .map_err(|_| "Firebase unavailable")?;
                firebase_client
                    .get_genre_tracker()
                    .await
                    .map_err(|_| "Failed to get genre_tracker from Firebase")
            })
        })
        .map_err(|_: &str| error_stack::Report::new(DjWizardLogError))?)
    }

    fn read_log() -> DjWizardLogResult<Self> {
        let soundeo_user = SoundeoUser::new().change_context(DjWizardLogError)?;
        let soundeo_log_path = Self::get_log_path(&soundeo_user);
        let soundeo_log_file_path = Path::new(&soundeo_log_path);
        let soundeo_log: Self = if soundeo_log_file_path.is_file() {
            let log_content = read_to_string(&soundeo_log_file_path)
                .into_report()
                .change_context(DjWizardLogError)?;

            serde_json::from_str::<Self>(&log_content)
                .into_report()
                .change_context(DjWizardLogError)?
        } else {
            Self {
                last_update: 0,
                queued_tracks: Vec::new(),
                soundeo: Soundeo::new(),
                spotify: Spotify::new(),
                available_tracks: HashSet::new(),
                url_list: HashSet::new(),
                genre_tracker: GenreTracker::new(),
                artist_manager: ArtistManager::new(),
                firebase_migrated_tracks: None,
                firebase_migrated_queues: None,
            }
        };
        Ok(soundeo_log)
    }

    fn save_log(&self) -> DjWizardLogResult<()> {
        let soundeo_user = SoundeoUser::new().change_context(DjWizardLogError)?;
        let save_log_string = serde_json::to_string_pretty(self)
            .into_report()
            .change_context(DjWizardLogError)?;
        let log_path = Self::get_log_path(&soundeo_user);
        fs::write(log_path, &save_log_string)
            .into_report()
            .change_context(DjWizardLogError)?;
        Ok(())
    }

    fn get_log_path(soundeo_user: &SoundeoUser) -> String {
        format!("{}/soundeo_log.json", soundeo_user.download_path)
    }

    pub fn get_log_path_from_config(user: &User) -> DjWizardLogResult<String> {
        if user.download_path.is_empty() {
            return Err(Report::new(DjWizardLogError)
                .attach_printable("Download path is not set in the configuration."));
        }
        Ok(format!("{}/soundeo_log.json", user.download_path))
    }

    pub fn add_queued_track(track_id: String, priority: Priority) -> DjWizardLogResult<bool> {
        // Use Firebase only - no fallback to local
        Ok(tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                // Load auth token
                let auth_token = GoogleAuth::load_token().await.map_err(|_| "No auth")?;

                // Create Firebase client
                let firebase_client = FirebaseClient::new(auth_token)
                    .await
                    .map_err(|_| "Firebase unavailable")?;

                // Add track to Firebase queue
                firebase_client
                    .add_queued_track(&track_id, priority)
                    .await
                    .map_err(|_| "Failed to add track to Firebase queue")
            })
        })
        .map_err(|_: &str| error_stack::Report::new(DjWizardLogError))?)
    }

    pub fn promote_tracks_to_top(track_ids_to_promote: &[String]) -> DjWizardLogResult<()> {
        // Use Firebase only - no fallback to local
        Ok(tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                // Load auth token
                let auth_token = GoogleAuth::load_token().await.map_err(|_| "No auth")?;

                // Create Firebase client
                let firebase_client = FirebaseClient::new(auth_token)
                    .await
                    .map_err(|_| "Firebase unavailable")?;

                // For each track to promote, update its priority to High
                for track_id in track_ids_to_promote {
                    let _ = firebase_client
                        .update_queued_track_priority(track_id, Priority::High)
                        .await;
                }

                println!(
                    "{}",
                    format!(
                        "{} tracks have been moved to high priority.",
                        track_ids_to_promote.len()
                    )
                    .green()
                );
                Ok(())
            })
        })
        .map_err(|_: &str| error_stack::Report::new(DjWizardLogError))?)
    }

    pub fn remove_queued_track(track_id: String) -> DjWizardLogResult<bool> {
        // Use Firebase only - no fallback to local
        Ok(tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                // Load auth token
                let auth_token = GoogleAuth::load_token().await.map_err(|_| "No auth")?;

                // Create Firebase client
                let firebase_client = FirebaseClient::new(auth_token)
                    .await
                    .map_err(|_| "Firebase unavailable")?;

                // Remove track from Firebase queue
                firebase_client
                    .remove_queued_track(&track_id)
                    .await
                    .map_err(|_| "Failed to remove track from Firebase queue")
            })
        })
        .map_err(|_: &str| error_stack::Report::new(DjWizardLogError))?)
    }

    pub fn add_available_track(track_id: String) -> DjWizardLogResult<bool> {
        // Use Firebase only - no fallback to local
        Ok(tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                // Load auth token
                let auth_token = GoogleAuth::load_token().await.map_err(|_| "No auth")?;

                // Create Firebase client
                let firebase_client = FirebaseClient::new(auth_token)
                    .await
                    .map_err(|_| "Firebase unavailable")?;

                // Add track to Firebase available tracks
                firebase_client
                    .add_available_track(&track_id)
                    .await
                    .map_err(|_| "Failed to add track to Firebase")
            })
        })
        .map_err(|_: &str| error_stack::Report::new(DjWizardLogError))?)
    }

    pub fn remove_available_track(track_id: String) -> DjWizardLogResult<bool> {
        // Use Firebase only - no local storage
        Ok(tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let auth_token = GoogleAuth::load_token().await.map_err(|_| "No auth")?;
                let firebase_client = FirebaseClient::new(auth_token)
                    .await
                    .map_err(|_| "Firebase unavailable")?;
                firebase_client
                    .remove_available_track(&track_id)
                    .await
                    .map_err(|_| "Failed to remove track from Firebase")
            })
        })
        .map_err(|_: &str| error_stack::Report::new(DjWizardLogError))?)
    }

    pub fn upload_to_ipfs() -> DjWizardLogResult<()> {
        println!("Saving the log file to IPFS");
        let soundeo_user = SoundeoUser::new().change_context(DjWizardLogError)?;
        let log_path = Self::get_log_path(&soundeo_user);
        let form = Form::new()
            .file("soundeo_log.json", log_path)
            .into_report()
            .change_context(DjWizardLogError)?;

        let mut config_file = User::new();
        config_file
            .read_config_file()
            .change_context(DjWizardLogError)?;
        let IPFSConfig {
            api_key,
            api_key_secret,
            ..
        } = config_file.ipfs.clone();

        let client = Client::builder()
            .timeout(Duration::from_secs(600))
            .build()
            .into_report()
            .change_context(DjWizardLogError)?;

        let response = client
            .post("https://ipfs.infura.io:5001/api/v0/add")
            .multipart(form)
            .basic_auth(api_key, Some(api_key_secret))
            .send()
            .into_report()
            .change_context(DjWizardLogError)?;
        let resp_text = response
            .text()
            .into_report()
            .change_context(DjWizardLogError)?;
        let value: Value = serde_json::from_str(&resp_text)
            .into_report()
            .change_context(DjWizardLogError)?;
        let hash = value["Hash"].clone().as_str().unwrap().to_string();
        config_file.ipfs.last_ipfs_hash = hash.clone();
        config_file
            .save_config_file()
            .change_context(DjWizardLogError)?;
        println!(
            "Log file successfully stored to IPFS with hash {}",
            hash.green()
        );
        Ok(())
    }

    pub fn update_spotify_soundeo_pairs(
        new_pairs: &HashMap<String, Option<String>>,
    ) -> DjWizardLogResult<()> {
        if new_pairs.is_empty() {
            return Ok(());
        }

        // Use Firebase only - no local storage
        Ok(tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let auth_token = GoogleAuth::load_token().await.map_err(|_| "No auth")?;
                let firebase_client = FirebaseClient::new(auth_token)
                    .await
                    .map_err(|_| "Firebase unavailable")?;

                // Get current spotify data, update pairs, and save back
                let mut spotify = firebase_client
                    .get_spotify()
                    .await
                    .map_err(|_| "Failed to get spotify from Firebase")?;
                spotify
                    .soundeo_track_ids
                    .extend(new_pairs.iter().map(|(k, v)| (k.clone(), v.clone())));
                firebase_client
                    .save_spotify(&spotify)
                    .await
                    .map_err(|_| "Failed to save spotify to Firebase")
            })
        })
        .map_err(|_: &str| error_stack::Report::new(DjWizardLogError))?)
    }

    /// Mark a queued track as migrated
    pub fn mark_queued_track_as_migrated(track_id: &str) -> DjWizardLogResult<()> {
        let mut log = Self::read_log()?;

        if let Some(track) = log
            .queued_tracks
            .iter_mut()
            .find(|t| t.track_id == track_id)
        {
            track.migrated = true;
            log.last_update = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .into_report()
                .change_context(DjWizardLogError)?
                .as_secs();
            log.save_log()?;
        }

        Ok(())
    }
}

impl SoundeoCRUD for DjWizardLog {
    fn create_soundeo_track(soundeo_track: SoundeoTrack) -> DjWizardLogResult<()> {
        // Use Firebase only - save individual track to soundeo_tracks collection
        Ok(tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let auth_token = GoogleAuth::load_token().await.map_err(|_| "No auth")?;
                let firebase_client = FirebaseClient::new(auth_token)
                    .await
                    .map_err(|_| "Firebase unavailable")?;

                // Save individual track to soundeo_tracks collection
                firebase_client
                    .save_soundeo_track(&soundeo_track)
                    .await
                    .map_err(|_| "Failed to save soundeo track to Firebase")
            })
        })
        .map_err(|_: &str| error_stack::Report::new(DjWizardLogError))?)
    }

    fn mark_track_as_downloaded(soundeo_track_id: String) -> DjWizardLogResult<()> {
        // Use Firebase only - get individual track, update, and save back
        Ok(tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let auth_token = GoogleAuth::load_token().await.map_err(|_| "No auth")?;
                let firebase_client = FirebaseClient::new(auth_token)
                    .await
                    .map_err(|_| "Firebase unavailable")?;

                // Get individual track from Firebase
                if let Some(mut track) = firebase_client
                    .get_soundeo_track(&soundeo_track_id)
                    .await
                    .map_err(|_| "Failed to get soundeo track from Firebase")?
                {
                    track.already_downloaded = true;
                    firebase_client
                        .save_soundeo_track(&track)
                        .await
                        .map_err(|_| "Failed to save soundeo track to Firebase")?;
                }
                Ok(())
            })
        })
        .map_err(|_: &str| error_stack::Report::new(DjWizardLogError))?)
    }

    fn reset_track_already_downloaded(soundeo_track_id: String) -> DjWizardLogResult<()> {
        // Use Firebase only - get individual track, update, and save back
        Ok(tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let auth_token = GoogleAuth::load_token().await.map_err(|_| "No auth")?;
                let firebase_client = FirebaseClient::new(auth_token)
                    .await
                    .map_err(|_| "Firebase unavailable")?;

                // Get individual track from Firebase
                if let Some(mut track) = firebase_client
                    .get_soundeo_track(&soundeo_track_id)
                    .await
                    .map_err(|_| "Failed to get soundeo track from Firebase")?
                {
                    track.already_downloaded = false;
                    firebase_client
                        .save_soundeo_track(&track)
                        .await
                        .map_err(|_| "Failed to save soundeo track to Firebase")?;
                }
                Ok(())
            })
        })
        .map_err(|_: &str| error_stack::Report::new(DjWizardLogError))?)
    }
}

impl SpotifyCRUD for DjWizardLog {
    fn update_spotify_playlist(spotify_playlist: SpotifyPlaylist) -> DjWizardLogResult<()> {
        // Use Firebase only - no local storage
        Ok(tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let auth_token = GoogleAuth::load_token().await.map_err(|_| "No auth")?;
                let firebase_client = FirebaseClient::new(auth_token)
                    .await
                    .map_err(|_| "Firebase unavailable")?;

                // Get current spotify data, update playlist, and save back
                let mut spotify = firebase_client
                    .get_spotify()
                    .await
                    .map_err(|_| "Failed to get spotify from Firebase")?;
                spotify.playlists.insert(
                    spotify_playlist.spotify_playlist_id.clone(),
                    spotify_playlist,
                );
                firebase_client
                    .save_spotify(&spotify)
                    .await
                    .map_err(|_| "Failed to save spotify to Firebase")
            })
        })
        .map_err(|_: &str| error_stack::Report::new(DjWizardLogError))?)
    }

    fn update_spotify_to_soundeo_track(
        spotify_track_id: String,
        soundeo_track_id: Option<String>,
    ) -> DjWizardLogResult<()> {
        // Use Firebase only - no local storage
        Ok(tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let auth_token = GoogleAuth::load_token().await.map_err(|_| "No auth")?;
                let firebase_client = FirebaseClient::new(auth_token)
                    .await
                    .map_err(|_| "Firebase unavailable")?;

                // Get current spotify data, update track mapping, and save back
                let mut spotify = firebase_client
                    .get_spotify()
                    .await
                    .map_err(|_| "Failed to get spotify from Firebase")?;
                spotify
                    .soundeo_track_ids
                    .insert(spotify_track_id, soundeo_track_id);
                firebase_client
                    .save_spotify(&spotify)
                    .await
                    .map_err(|_| "Failed to save spotify to Firebase")
            })
        })
        .map_err(|_: &str| error_stack::Report::new(DjWizardLogError))?)
    }

    fn delete_spotify_playlists(playlist_ids: &[String]) -> DjWizardLogResult<()> {
        // Use Firebase only - no local storage
        Ok(tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let auth_token = GoogleAuth::load_token().await.map_err(|_| "No auth")?;
                let firebase_client = FirebaseClient::new(auth_token)
                    .await
                    .map_err(|_| "Firebase unavailable")?;

                // Get current spotify data, delete playlists, and save back
                let mut spotify = firebase_client
                    .get_spotify()
                    .await
                    .map_err(|_| "Failed to get spotify from Firebase")?;
                let mut deleted_count = 0;

                for id in playlist_ids {
                    if spotify.playlists.remove(id).is_some() {
                        deleted_count += 1;
                    }
                }

                if deleted_count > 0 {
                    firebase_client
                        .save_spotify(&spotify)
                        .await
                        .map_err(|_| "Failed to save spotify to Firebase")?;
                }
                Ok(())
            })
        })
        .map_err(|_: &str| error_stack::Report::new(DjWizardLogError))?)
    }

    fn add_to_multiple_matches_cache(
        spotify_id: String,
        results: Vec<SoundeoSearchBarResult>,
    ) -> DjWizardLogResult<()> {
        // Use Firebase only - no local storage
        Ok(tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let auth_token = GoogleAuth::load_token().await.map_err(|_| "No auth")?;
                let firebase_client = FirebaseClient::new(auth_token)
                    .await
                    .map_err(|_| "Firebase unavailable")?;

                // Get current spotify data, update cache, and save back
                let mut spotify = firebase_client
                    .get_spotify()
                    .await
                    .map_err(|_| "Failed to get spotify from Firebase")?;
                spotify.multiple_matches_cache.insert(spotify_id, results);
                firebase_client
                    .save_spotify(&spotify)
                    .await
                    .map_err(|_| "Failed to save spotify to Firebase")
            })
        })
        .map_err(|_: &str| error_stack::Report::new(DjWizardLogError))?)
    }
}

impl UrlListCRUD for DjWizardLog {
    fn add_url_to_url_list(soundeo_url: url::Url) -> DjWizardLogResult<bool> {
        // Use Firebase only - no local storage
        Ok(tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let auth_token = GoogleAuth::load_token().await.map_err(|_| "No auth")?;
                let firebase_client = FirebaseClient::new(auth_token)
                    .await
                    .map_err(|_| "Firebase unavailable")?;

                // Get current url_list, add URL, and save back
                let mut url_list = firebase_client
                    .get_url_list()
                    .await
                    .map_err(|_| "Failed to get url_list from Firebase")?;
                let result = url_list.insert(soundeo_url.to_string());
                firebase_client
                    .save_url_list(&url_list)
                    .await
                    .map_err(|_| "Failed to save url_list to Firebase")?;
                Ok(result)
            })
        })
        .map_err(|_: &str| error_stack::Report::new(DjWizardLogError))?)
    }

    fn remove_url_from_url_list(soundeo_url: String) -> DjWizardLogResult<bool> {
        // Use Firebase only - no local storage
        Ok(tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let auth_token = GoogleAuth::load_token().await.map_err(|_| "No auth")?;
                let firebase_client = FirebaseClient::new(auth_token)
                    .await
                    .map_err(|_| "Firebase unavailable")?;

                // Get current url_list, remove URL, and save back
                let mut url_list = firebase_client
                    .get_url_list()
                    .await
                    .map_err(|_| "Failed to get url_list from Firebase")?;
                let result = url_list.remove(&soundeo_url);
                firebase_client
                    .save_url_list(&url_list)
                    .await
                    .map_err(|_| "Failed to save url_list to Firebase")?;
                Ok(result)
            })
        })
        .map_err(|_: &str| error_stack::Report::new(DjWizardLogError))?)
    }
}

impl GenreTrackerCRUD for DjWizardLog {
    fn get_genre_tracker() -> DjWizardLogResult<GenreTracker> {
        // Use Firebase only - no local storage
        Ok(tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let auth_token = GoogleAuth::load_token().await.map_err(|_| "No auth")?;
                let firebase_client = FirebaseClient::new(auth_token)
                    .await
                    .map_err(|_| "Firebase unavailable")?;
                firebase_client
                    .get_genre_tracker()
                    .await
                    .map_err(|_| "Failed to get genre_tracker from Firebase")
            })
        })
        .map_err(|_: &str| error_stack::Report::new(DjWizardLogError))?)
    }

    fn save_genre_tracker(tracker: GenreTracker) -> DjWizardLogResult<()> {
        // Use Firebase only - no local storage
        Ok(tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let auth_token = GoogleAuth::load_token().await.map_err(|_| "No auth")?;
                let firebase_client = FirebaseClient::new(auth_token)
                    .await
                    .map_err(|_| "Firebase unavailable")?;
                firebase_client
                    .save_genre_tracker(&tracker)
                    .await
                    .map_err(|_| "Failed to save genre_tracker to Firebase")
            })
        })
        .map_err(|_: &str| error_stack::Report::new(DjWizardLogError))?)
    }
}

impl ArtistCRUD for DjWizardLog {
    fn get_artist_manager() -> DjWizardLogResult<ArtistManager> {
        // Use tokio::task::block_in_place for compatibility with sync methods
        Ok(tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                // Load auth token
                let auth_token = GoogleAuth::load_token().await.map_err(|_| "No auth")?;

                // Create Firebase client
                let firebase_client = FirebaseClient::new(auth_token)
                    .await
                    .map_err(|_| "Firebase unavailable")?;

                // Get artists from Firebase - no fallback
                match firebase_client.load_artists().await {
                    Ok(Some(artist_manager)) => Ok(artist_manager),
                    Ok(None) => Ok(ArtistManager::default()), // Return empty manager if no data
                    Err(_) => Err("Failed to get artists from Firebase")
                }
            })
        })
        .map_err(|_: &str| error_stack::Report::new(DjWizardLogError))?)
    }

    fn save_artist_manager(manager: ArtistManager) -> DjWizardLogResult<()> {
        // Use tokio::task::block_in_place for compatibility with sync methods
        Ok(tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                // Load auth token
                let auth_token = GoogleAuth::load_token().await.map_err(|_| "No auth")?;

                // Create Firebase client
                let firebase_client = FirebaseClient::new(auth_token)
                    .await
                    .map_err(|_| "Firebase unavailable")?;

                // Save to Firebase - no fallback
                firebase_client.save_artists(&manager).await.map_err(|_| "Failed to save artists to Firebase")
            
            })
        })
        .map_err(|_: &str| error_stack::Report::new(DjWizardLogError))?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_upload_to_ipfs() {
        DjWizardLog::upload_to_ipfs().unwrap();
    }
}
