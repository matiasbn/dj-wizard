use std::collections::{HashMap, HashSet};
use std::fs::read_to_string;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use std::{fmt, fs};

use crate::artist::{ArtistCRUD, ArtistManager};
use crate::url_list::UrlListCRUD;
use colored::Colorize;
use error_stack::{IntoReport, Report, ResultExt};
use reqwest::blocking::multipart::Form;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::soundeo::search_bar::SoundeoSearchBarResult;
use crate::soundeo::track::SoundeoTrack;
use crate::soundeo::{Soundeo, SoundeoCRUD};
use crate::spotify::playlist::SpotifyPlaylist;
use crate::spotify::{Spotify, SpotifyCRUD};
use crate::user::{IPFSConfig, SoundeoUser, User};
use crate::genre_tracker::{GenreTracker, GenreTrackerCRUD};

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
        let log = Self::read_log()?;
        Ok(log.queued_tracks)
    }

    pub fn get_available_tracks() -> DjWizardLogResult<HashSet<String>> {
        let log = Self::read_log()?;
        Ok(log.available_tracks)
    }

    pub fn get_spotify() -> DjWizardLogResult<Spotify> {
        let log = Self::read_log()?;
        Ok(log.spotify)
    }

    pub fn get_soundeo() -> DjWizardLogResult<Soundeo> {
        let log = Self::read_log()?;
        Ok(log.soundeo)
    }

    /// Get a single track - Firebase first, fallback to local JSON
    pub fn get_track_optimized(track_id: &str) -> DjWizardLogResult<Option<crate::soundeo::track::SoundeoTrack>> {
        // Try Firebase first (blocking async call)
        if let Ok(firebase_track) = Self::try_get_track_from_firebase(track_id) {
            return Ok(firebase_track);
        }
        
        // Fallback to local JSON
        let soundeo = Self::get_soundeo()?;
        Ok(soundeo.tracks_info.get(track_id).cloned())
    }

    /// Get multiple tracks - Firebase first, fallback to local JSON
    pub fn get_tracks_optimized(track_ids: &[String]) -> DjWizardLogResult<std::collections::HashMap<String, crate::soundeo::track::SoundeoTrack>> {
        // Try Firebase first (blocking async call)
        if let Ok(firebase_tracks) = Self::try_get_tracks_from_firebase(track_ids) {
            return Ok(firebase_tracks);
        }
        
        // Fallback to local JSON
        let soundeo = Self::get_soundeo()?;
        let mut tracks = std::collections::HashMap::new();
        for track_id in track_ids {
            if let Some(track) = soundeo.tracks_info.get(track_id) {
                tracks.insert(track_id.clone(), track.clone());
            }
        }
        Ok(tracks)
    }

    fn try_get_track_from_firebase(track_id: &str) -> Result<Option<crate::soundeo::track::SoundeoTrack>, Box<dyn std::error::Error>> {
        use crate::auth::{firebase_client::FirebaseClient, google_auth::GoogleAuth};
        
        // Use tokio::task::block_in_place for compatibility with sync methods
        Ok(tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                // Load auth token
                let auth_token = GoogleAuth::load_token().map_err(|_| "No auth")?;
                
                // Create Firebase client
                let firebase_client = FirebaseClient::new(auth_token).await.map_err(|_| "Firebase unavailable")?;
                
                // Get track from Firebase
                firebase_client.get_track(track_id).await.map_err(|_| "Track fetch failed")
            })
        })?)
    }

    fn try_get_tracks_from_firebase(track_ids: &[String]) -> Result<std::collections::HashMap<String, crate::soundeo::track::SoundeoTrack>, Box<dyn std::error::Error>> {
        use crate::auth::{firebase_client::FirebaseClient, google_auth::GoogleAuth};
        
        // Use tokio::task::block_in_place for compatibility with sync methods
        Ok(tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                // Load auth token
                let auth_token = GoogleAuth::load_token().map_err(|_| "No auth")?;
                
                // Create Firebase client
                let firebase_client = FirebaseClient::new(auth_token).await.map_err(|_| "Firebase unavailable")?;
                
                // Get tracks from Firebase
                firebase_client.get_tracks(track_ids).await.map_err(|_| "Tracks fetch failed")
            })
        })?)
    }

    pub fn get_url_list() -> DjWizardLogResult<HashSet<String>> {
        let log = Self::read_log()?;
        Ok(log.url_list)
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
        let log = Self::read_log()?;
        Ok(log.genre_tracker)
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
        let mut log = Self::read_log()?;
        log.last_update = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .into_report()
            .change_context(DjWizardLogError)?
            .as_secs();

        if log.queued_tracks.iter().any(|t| t.track_id == track_id) {
            return Ok(false);
        }

        let max_order_key_in_group = log
            .queued_tracks
            .iter()
            .filter(|t| t.priority == priority)
            .map(|t| t.order_key)
            .fold(f64::NEG_INFINITY, f64::max);

        let new_track = QueuedTrack {
            track_id,
            priority,
            order_key: if max_order_key_in_group.is_finite() {
                max_order_key_in_group + 1.0
            } else {
                1.0
            },
            added_at: log.last_update,
            migrated: false,
        };
        log.queued_tracks.push(new_track);
        log.save_log()?;
        Ok(true)
    }

    pub fn promote_tracks_to_top(track_ids_to_promote: &[String]) -> DjWizardLogResult<()> {
        let mut log = Self::read_log()?;

        // 1. Find the lowest (most priority) order_key among existing High priority tracks.
        let min_high_priority_key = log
            .queued_tracks
            .iter()
            .filter(|t| t.priority == Priority::High)
            .map(|t| t.order_key)
            .fold(f64::INFINITY, f64::min);

        // 2. Determine the starting point for the new order_keys.
        // If no 'High' tracks exist, we can start from 0. Otherwise, start below the current minimum.
        let mut next_order_key = if min_high_priority_key.is_finite() {
            min_high_priority_key - 1.0
        } else {
            0.0
        };

        // 3. Update the selected tracks.
        for track in log.queued_tracks.iter_mut() {
            if track_ids_to_promote.contains(&track.track_id) {
                track.priority = Priority::High;
                track.order_key = next_order_key;
                next_order_key -= 1.0; // Each subsequent promoted track gets an even lower key.
            }
        }

        log.save_log()?;
        println!(
            "{}",
            format!(
                "{} tracks have been moved to the top of the queue.",
                track_ids_to_promote.len()
            )
            .green()
        );
        Ok(())
    }

    pub fn remove_queued_track(track_id: String) -> DjWizardLogResult<bool> {
        let mut log = Self::read_log()?;
        log.last_update = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .into_report()
            .change_context(DjWizardLogError)?
            .as_secs();
        let initial_len = log.queued_tracks.len();
        log.queued_tracks.retain(|t| t.track_id != track_id);
        let was_removed = log.queued_tracks.len() < initial_len;
        if was_removed {
            log.save_log()?;
        }
        Ok(was_removed)
    }

    pub fn add_available_track(track_id: String) -> DjWizardLogResult<bool> {
        let mut log = Self::read_log()?;
        log.last_update = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .into_report()
            .change_context(DjWizardLogError)?
            .as_secs();
        let result = log.available_tracks.insert(track_id);
        log.save_log()?;
        Ok(result)
    }

    pub fn remove_available_track(track_id: String) -> DjWizardLogResult<bool> {
        let mut log = Self::read_log()?;
        log.last_update = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .into_report()
            .change_context(DjWizardLogError)?
            .as_secs();
        let result = log.available_tracks.remove(&track_id);
        log.save_log()?;
        Ok(result)
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
        let mut log = Self::read_log()?;
        log.spotify
            .soundeo_track_ids
            .extend(new_pairs.iter().map(|(k, v)| (k.clone(), v.clone())));
        log.save_log()?;
        Ok(())
    }

    /// Mark a queued track as migrated
    pub fn mark_queued_track_as_migrated(track_id: &str) -> DjWizardLogResult<()> {
        let mut log = Self::read_log()?;
        
        if let Some(track) = log.queued_tracks.iter_mut().find(|t| t.track_id == track_id) {
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
        let mut log = Self::read_log()?;
        log.soundeo
            .tracks_info
            .insert(soundeo_track.id.clone(), soundeo_track.clone());
        log.save_log()?;
        Ok(())
    }

    fn mark_track_as_downloaded(soundeo_track_id: String) -> DjWizardLogResult<()> {
        let mut log = Self::read_log()?;
        log.last_update = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .into_report()
            .change_context(DjWizardLogError)?
            .as_secs();
        log.soundeo
            .tracks_info
            .get_mut(&soundeo_track_id)
            .ok_or(DjWizardLogError)
            .into_report()?
            .already_downloaded = true;
        log.save_log()?;
        Ok(())
    }

    fn reset_track_already_downloaded(soundeo_track_id: String) -> DjWizardLogResult<()> {
        let mut log = Self::read_log()?;
        log.last_update = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .into_report()
            .change_context(DjWizardLogError)?
            .as_secs();
        log.soundeo
            .tracks_info
            .get_mut(&soundeo_track_id)
            .ok_or(DjWizardLogError)
            .into_report()?
            .already_downloaded = false;
        log.save_log()?;
        Ok(())
    }
}

impl SpotifyCRUD for DjWizardLog {
    fn update_spotify_playlist(spotify_playlist: SpotifyPlaylist) -> DjWizardLogResult<()> {
        let mut log = Self::read_log()?;
        // Using insert will either add a new playlist or update an existing one.
        log.spotify.playlists.insert(
            spotify_playlist.spotify_playlist_id.clone(),
            spotify_playlist,
        );
        log.save_log()?;
        Ok(())
    }

    fn update_spotify_to_soundeo_track(
        spotify_track_id: String,
        soundeo_track_id: Option<String>,
    ) -> DjWizardLogResult<()> {
        let mut log = Self::read_log()?;
        log.spotify
            .soundeo_track_ids
            .insert(spotify_track_id, soundeo_track_id.clone());
        log.save_log()?;
        Ok(())
    }

    fn delete_spotify_playlists(playlist_ids: &[String]) -> DjWizardLogResult<()> {
        let mut log = Self::read_log()?;
        let mut deleted_count = 0;

        for id in playlist_ids {
            if log.spotify.playlists.remove(id).is_some() {
                deleted_count += 1;
            }
        }

        if deleted_count > 0 {
            log.save_log()?;
        }
        Ok(())
    }

    fn add_to_multiple_matches_cache(
        spotify_id: String,
        results: Vec<SoundeoSearchBarResult>,
    ) -> DjWizardLogResult<()> {
        let mut log = Self::read_log()?;
        log.spotify
            .multiple_matches_cache
            .insert(spotify_id, results);
        log.save_log()
    }
}

impl UrlListCRUD for DjWizardLog {
    fn add_url_to_url_list(soundeo_url: url::Url) -> DjWizardLogResult<bool> {
        let mut log = Self::read_log()?;
        log.last_update = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .into_report()
            .change_context(DjWizardLogError)?
            .as_secs();
        let result = log.url_list.insert(soundeo_url.to_string());
        log.save_log()?;
        Ok(result)
    }

    fn remove_url_from_url_list(soundeo_url: String) -> DjWizardLogResult<bool> {
        let mut log = Self::read_log()?;
        log.last_update = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .into_report()
            .change_context(DjWizardLogError)?
            .as_secs();
        let result = log.url_list.remove(&soundeo_url);
        log.save_log()?;
        Ok(result)
    }
}

impl GenreTrackerCRUD for DjWizardLog {
    fn get_genre_tracker() -> DjWizardLogResult<GenreTracker> {
        let log = Self::read_log()?;
        Ok(log.genre_tracker)
    }

    fn save_genre_tracker(tracker: GenreTracker) -> DjWizardLogResult<()> {
        let mut log = Self::read_log()?;
        log.genre_tracker = tracker;
        log.last_update = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .into_report()
            .change_context(DjWizardLogError)?
            .as_secs();
        log.save_log()?;
        Ok(())
    }
}

impl ArtistCRUD for DjWizardLog {
    fn get_artist_manager() -> DjWizardLogResult<ArtistManager> {
        let log = Self::read_log()?;
        Ok(log.artist_manager)
    }

    fn save_artist_manager(manager: ArtistManager) -> DjWizardLogResult<()> {
        let mut log = Self::read_log()?;
        log.last_update = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .into_report()
            .change_context(DjWizardLogError)?
            .as_secs();
        log.artist_manager = manager;
        log.save_log()?;
        Ok(())
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
