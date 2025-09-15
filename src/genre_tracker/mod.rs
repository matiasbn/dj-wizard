use std::collections::HashMap;
use std::fmt;

use chrono::Utc;
use error_stack::{IntoReport, ResultExt};
use serde::{Deserialize, Serialize};

use crate::log::DjWizardLogResult;

pub mod commands;
pub mod scraper;

#[derive(Debug)]
pub struct GenreTrackerError;

impl fmt::Display for GenreTrackerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Genre tracker error")
    }
}

impl std::error::Error for GenreTrackerError {}

pub type GenreTrackerResult<T> = error_stack::Result<T, GenreTrackerError>;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GenreInfo {
    pub id: u32,
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TrackedGenre {
    pub genre_id: u32,
    pub genre_name: String,
    pub last_checked_date: String,
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GenreTracker {
    pub tracked_genres: HashMap<u32, TrackedGenre>,
    pub available_genres: HashMap<u32, GenreInfo>,
}

impl Default for GenreTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl GenreTracker {
    pub fn new() -> Self {
        let mut available_genres = HashMap::new();
        
        // Populate with known Soundeo genres
        available_genres.insert(1, GenreInfo { id: 1, name: "Drum and Bass".to_string() });
        available_genres.insert(2, GenreInfo { id: 2, name: "Dubstep".to_string() });
        available_genres.insert(3, GenreInfo { id: 3, name: "Techno".to_string() });
        available_genres.insert(4, GenreInfo { id: 4, name: "House".to_string() });
        available_genres.insert(5, GenreInfo { id: 5, name: "Trance".to_string() });
        available_genres.insert(6, GenreInfo { id: 6, name: "Hardcore".to_string() });
        available_genres.insert(7, GenreInfo { id: 7, name: "Breakbeat".to_string() });
        available_genres.insert(8, GenreInfo { id: 8, name: "Electro".to_string() });
        available_genres.insert(9, GenreInfo { id: 9, name: "Minimal".to_string() });
        available_genres.insert(10, GenreInfo { id: 10, name: "Progressive".to_string() });
        available_genres.insert(11, GenreInfo { id: 11, name: "Psy-Trance".to_string() });
        available_genres.insert(12, GenreInfo { id: 12, name: "Hip-Hop".to_string() });
        available_genres.insert(13, GenreInfo { id: 13, name: "Reggae / Dub".to_string() });
        available_genres.insert(14, GenreInfo { id: 14, name: "Other".to_string() });

        Self {
            tracked_genres: HashMap::new(),
            available_genres,
        }
    }

    pub fn add_tracked_genre(&mut self, genre_id: u32) -> GenreTrackerResult<()> {
        if let Some(genre_info) = self.available_genres.get(&genre_id) {
            let now = Utc::now().format("%Y-%m-%d").to_string();
            let tracked_genre = TrackedGenre {
                genre_id,
                genre_name: genre_info.name.clone(),
                last_checked_date: now.clone(),
                created_at: now,
            };
            self.tracked_genres.insert(genre_id, tracked_genre);
            Ok(())
        } else {
            Err(GenreTrackerError).into_report()
        }
    }

    pub fn update_last_checked(&mut self, genre_id: u32) -> GenreTrackerResult<()> {
        if let Some(tracked_genre) = self.tracked_genres.get_mut(&genre_id) {
            tracked_genre.last_checked_date = Utc::now().format("%Y-%m-%d").to_string();
            Ok(())
        } else {
            Err(GenreTrackerError).into_report()
        }
    }

    pub fn build_soundeo_url(&self, genre_id: u32, start_date: &str, end_date: &str, page: u32) -> String {
        format!(
            "https://soundeo.com/list/tracks?availableFilter=1&genreFilter={}&timeFilter=r_{}_{}&page={}",
            genre_id, start_date, end_date, page
        )
    }
}

pub trait GenreTrackerCRUD {
    fn get_genre_tracker() -> DjWizardLogResult<GenreTracker>;
    fn save_genre_tracker(tracker: GenreTracker) -> DjWizardLogResult<()>;
}