use std::{fmt, collections::HashMap};

use chrono::Utc;
use error_stack::{IntoReport, ResultExt};
use serde::{Deserialize, Serialize};

use crate::log::DjWizardLogResult;

pub mod commands;

#[derive(Debug)]
pub struct ArtistError;

impl fmt::Display for ArtistError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Artist error")
    }
}

impl std::error::Error for ArtistError {}

pub type ArtistResult<T> = error_stack::Result<T, ArtistError>;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Artist {
    pub name: String,
    pub genres: Vec<String>, // Genres this artist is associated with
    pub created_at: String,
    pub last_updated: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ArtistManager {
    pub favorite_artists: HashMap<String, Artist>, // Key: artist name (normalized)
}

impl Default for ArtistManager {
    fn default() -> Self {
        Self {
            favorite_artists: HashMap::new(),
        }
    }
}

impl ArtistManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_artist(&mut self, artist_name: &str, genre: Option<&str>) -> ArtistResult<bool> {
        let normalized_name = Self::normalize_name(artist_name);
        let now = Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();

        match self.favorite_artists.get_mut(&normalized_name) {
            Some(existing_artist) => {
                // Artist exists, add genre if provided and not already present
                if let Some(genre_name) = genre {
                    if !existing_artist.genres.contains(&genre_name.to_string()) {
                        existing_artist.genres.push(genre_name.to_string());
                        existing_artist.last_updated = now;
                        return Ok(true); // Updated
                    }
                }
                Ok(false) // No change
            }
            None => {
                // New artist
                let mut genres = Vec::new();
                if let Some(genre_name) = genre {
                    genres.push(genre_name.to_string());
                }

                let artist = Artist {
                    name: artist_name.to_string(),
                    genres,
                    created_at: now.clone(),
                    last_updated: now,
                };

                self.favorite_artists.insert(normalized_name, artist);
                Ok(true) // Added
            }
        }
    }

    pub fn remove_artist(&mut self, artist_name: &str) -> ArtistResult<bool> {
        let normalized_name = Self::normalize_name(artist_name);
        Ok(self.favorite_artists.remove(&normalized_name).is_some())
    }

    pub fn remove_artist_from_genre(&mut self, artist_name: &str, genre: &str) -> ArtistResult<bool> {
        let normalized_name = Self::normalize_name(artist_name);
        
        if let Some(artist) = self.favorite_artists.get_mut(&normalized_name) {
            let before_len = artist.genres.len();
            artist.genres.retain(|g| g != genre);
            let after_len = artist.genres.len();
            
            if before_len != after_len {
                artist.last_updated = Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
                
                // If no genres left, remove the artist entirely
                if artist.genres.is_empty() {
                    self.favorite_artists.remove(&normalized_name);
                }
                
                Ok(true)
            } else {
                Ok(false)
            }
        } else {
            Ok(false)
        }
    }

    pub fn get_artist(&self, artist_name: &str) -> Option<&Artist> {
        let normalized_name = Self::normalize_name(artist_name);
        self.favorite_artists.get(&normalized_name)
    }

    pub fn get_artists_by_genre(&self, genre: &str) -> Vec<&Artist> {
        self.favorite_artists
            .values()
            .filter(|artist| artist.genres.contains(&genre.to_string()))
            .collect()
    }

    pub fn get_all_artists(&self) -> Vec<&Artist> {
        self.favorite_artists.values().collect()
    }

    pub fn get_all_genres(&self) -> Vec<String> {
        let mut genres: Vec<String> = self.favorite_artists
            .values()
            .flat_map(|artist| artist.genres.iter())
            .cloned()
            .collect();
        
        genres.sort();
        genres.dedup();
        genres
    }

    pub fn search_artists(&self, query: &str) -> Vec<&Artist> {
        let query_lower = query.to_lowercase();
        self.favorite_artists
            .values()
            .filter(|artist| artist.name.to_lowercase().contains(&query_lower))
            .collect()
    }

    fn normalize_name(name: &str) -> String {
        name.trim().to_lowercase()
    }

    pub fn format_artist_name(name: &str) -> String {
        name.split_whitespace()
            .map(|word| {
                let mut chars = word.chars();
                match chars.next() {
                    None => String::new(),
                    Some(first) => first.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase(),
                }
            })
            .collect::<Vec<String>>()
            .join(" ")
    }
}

pub trait ArtistCRUD {
    fn get_artist_manager() -> DjWizardLogResult<ArtistManager>;
    fn save_artist_manager(manager: ArtistManager) -> DjWizardLogResult<()>;
}