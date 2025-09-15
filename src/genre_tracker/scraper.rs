use std::collections::HashMap;

use error_stack::{IntoReport, ResultExt};
use reqwest::Client;
use scraper::{Html, Selector};

use crate::genre_tracker::{GenreInfo, GenreTrackerError, GenreTrackerResult};
use crate::user::SoundeoUser;

pub struct GenreScraper;

impl GenreScraper {
    /// Scrapes the Soundeo website to get the list of available genres with their IDs
    pub async fn fetch_genres_from_soundeo(soundeo_user: &SoundeoUser) -> GenreTrackerResult<HashMap<u32, GenreInfo>> {
        let client = Client::new();
        let session_cookie = soundeo_user
            .get_session_cookie()
            .change_context(GenreTrackerError)?;
        
        // Visit the tracks page to get the genre dropdown
        let response = client
            .get("https://soundeo.com/list/tracks")
            .header("cookie", session_cookie)
            .send()
            .await
            .into_report()
            .change_context(GenreTrackerError)?
            .text()
            .await
            .into_report()
            .change_context(GenreTrackerError)?;
        
        let document = Html::parse_document(&response);
        
        // Look for genre filter select element
        // This selector might need adjustment based on actual HTML structure
        let genre_selector = Selector::parse("select[name='genreFilter'] option, select#genreFilter option, .genre-filter option")
            .map_err(|_| GenreTrackerError)
            .into_report()?;
        
        let mut genres = HashMap::new();
        
        for element in document.select(&genre_selector) {
            // Get the value attribute (genre ID)
            if let Some(value) = element.value().attr("value") {
                // Skip empty or "all" options
                if value.is_empty() || value == "0" || value == "all" {
                    continue;
                }
                
                // Parse the ID
                if let Ok(id) = value.parse::<u32>() {
                    // Get the text content (genre name)
                    let name = element.text().collect::<String>().trim().to_string();
                    
                    if !name.is_empty() {
                        genres.insert(id, GenreInfo { id, name });
                    }
                }
            }
        }
        
        // If we couldn't find genres from HTML, try the API approach
        if genres.is_empty() {
            println!("Could not find genres in HTML, trying alternative method...");
            genres = Self::fetch_genres_from_api(soundeo_user).await?;
        }
        
        Ok(genres)
    }
    
    /// Alternative method: Try to get genres from API or track listings
    async fn fetch_genres_from_api(soundeo_user: &SoundeoUser) -> GenreTrackerResult<HashMap<u32, GenreInfo>> {
        let mut genres = HashMap::new();
        
        // Try fetching pages with different genre IDs to discover valid ones
        // This is a fallback method - we test common IDs
        for test_id in 1..=20 {
            let client = Client::new();
            let session_cookie = soundeo_user
                .get_session_cookie()
                .change_context(GenreTrackerError)?;
            
            let url = format!(
                "https://soundeo.com/list/tracks?genreFilter={}&page=1",
                test_id
            );
            
            let response = client
                .get(&url)
                .header("cookie", &session_cookie)
                .send()
                .await
                .into_report()
                .change_context(GenreTrackerError)?;
            
            // If we get a successful response, this genre ID exists
            if response.status().is_success() {
                let text = response.text().await.into_report().change_context(GenreTrackerError)?;
                
                // Try to extract genre name from the page
                if let Some(genre_name) = Self::extract_genre_name_from_page(&text, test_id) {
                    genres.insert(test_id, GenreInfo { 
                        id: test_id, 
                        name: genre_name 
                    });
                    println!("Found genre ID {}: {}", test_id, genres[&test_id].name);
                }
            }
        }
        
        Ok(genres)
    }
    
    fn extract_genre_name_from_page(html: &str, genre_id: u32) -> Option<String> {
        // Try to find the genre name in the page
        // This would need to be adjusted based on actual HTML structure
        let document = Html::parse_document(html);
        
        // Look for selected option in genre filter
        let selector = Selector::parse(&format!("option[value='{}'][selected]", genre_id)).ok()?;
        
        for element in document.select(&selector) {
            let name = element.text().collect::<String>().trim().to_string();
            if !name.is_empty() {
                return Some(name);
            }
        }
        
        None
    }
}