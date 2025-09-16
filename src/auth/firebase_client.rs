use serde::{Deserialize, Serialize};
use serde_json::Value;
use reqwest::StatusCode;

use super::{AuthError, AuthResult, AuthToken};
use crate::artist::{Artist, ArtistManager};
use crate::config::AppConfig;
use crate::genre_tracker::{GenreInfo, GenreTracker, TrackedGenre};

#[derive(Debug)]
pub struct FirebaseClient {
    client: reqwest::Client,
    project_id: String,
    user_id: String,
    access_token: String,
    refresh_token: Option<String>,
    token_expires_at: chrono::DateTime<chrono::Utc>,
}

impl FirebaseClient {
    // Collection names constants
    const COLLECTION_DJ_WIZARD_DATA: &'static str = "dj_wizard_data";
    const DOCUMENT_MAIN: &'static str = "main";
    pub async fn new(auth_token: AuthToken) -> AuthResult<Self> {
        Ok(Self {
            client: reqwest::Client::new(),
            project_id: AppConfig::FIREBASE_PROJECT_ID.to_string(),
            user_id: auth_token.user_id,
            access_token: auth_token.access_token,
            refresh_token: auth_token.refresh_token,
            token_expires_at: auth_token.expires_at,
        })
    }

    /// Get the base URL for Firestore REST API
    fn firestore_url(&self) -> String {
        format!(
            "https://firestore.googleapis.com/v1/projects/{}/databases/(default)/documents",
            self.project_id
        )
    }

    /// Centralized Firebase collection paths
    fn get_collection_path(&self, collection: &str) -> String {
        format!(
            "{}/users/{}/{}",
            self.firestore_url(),
            urlencoding::encode(&self.user_id),
            collection
        )
    }

    fn get_document_path(&self, collection: &str, document_id: &str) -> String {
        format!(
            "{}/users/{}/{}/{}",
            self.firestore_url(),
            urlencoding::encode(&self.user_id),
            collection,
            document_id
        )
    }

    /// Get complete Firebase URL path based on collection type
    fn get_firebase_url(&self, collection_type: &str) -> String {
        match collection_type {
            Self::COLLECTION_DJ_WIZARD_DATA => {
                self.get_document_path(Self::COLLECTION_DJ_WIZARD_DATA, Self::DOCUMENT_MAIN)
            }
            _ => {
                // Default to collection path for unknown types
                self.get_collection_path(collection_type)
            }
        }
    }

    /// Ensure token is valid, refresh if needed
    async fn ensure_valid_token(&mut self) -> AuthResult<()> {
        use crate::auth::google_auth::GoogleAuth;

        // Check if token expires within 5 minutes
        let expires_soon = self.token_expires_at - chrono::Duration::minutes(5);
        if chrono::Utc::now() > expires_soon {
            let new_token = if let Some(ref refresh_token) = self.refresh_token {
                // Try automatic refresh first
                match GoogleAuth::refresh_token(refresh_token).await {
                    Ok(token) => token,
                    Err(_) => {
                        println!("🌐 Opening browser for authentication...");
                        GoogleAuth::new()
                            .login()
                            .await
                            .map_err(|_| AuthError::new("Failed to login"))?
                    }
                }
            } else {
                // No refresh token, do full login
                println!("🌐 Opening browser for authentication...");
                GoogleAuth::new()
                    .login()
                    .await
                    .map_err(|_| AuthError::new("Failed to login"))?
            };

            self.access_token = new_token.access_token;
            self.refresh_token = new_token.refresh_token;
            self.token_expires_at = new_token.expires_at;
        }

        Ok(())
    }

    /// Get the collection path for the current user
    fn user_collection_path(&self, collection: &str) -> String {
        format!("users/{}/{}", self.user_id, collection)
    }

    /// Create or update a document
    pub async fn set_document(
        &self,
        collection: &str,
        document_id: &str,
        data: &Value,
    ) -> AuthResult<()> {
        let url = format!(
            "{}/{}/{}",
            self.firestore_url(),
            self.user_collection_path(collection),
            document_id
        );

        let response = self
            .client
            .patch(&url)
            .header("Authorization", format!("Bearer {}", self.access_token))
            .json(&self.convert_to_firestore_document(data))
            .send()
            .await
            .map_err(|e| AuthError::new(&format!("Failed to set document: {}", e)))?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(AuthError::new(&format!("Firestore error: {}", error_text)).into());
        }

        Ok(())
    }

    /// Get a document
    pub async fn get_document(
        &self,
        collection: &str,
        document_id: &str,
    ) -> AuthResult<Option<Value>> {
        let url = format!(
            "{}/{}/{}",
            self.firestore_url(),
            self.user_collection_path(collection),
            document_id
        );

        let response = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.access_token))
            .send()
            .await
            .map_err(|e| AuthError::new(&format!("Failed to get document: {}", e)))?;

        if response.status() == 404 {
            return Ok(None);
        }

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(AuthError::new(&format!("Firestore error: {}", error_text)).into());
        }

        let doc: Value = response
            .json()
            .await
            .map_err(|e| AuthError::new(&format!("Failed to parse response: {}", e)))?;

        Ok(Some(self.convert_from_firestore_value(&doc)))
    }

    /// Convert JSON object to Firestore document format
    fn convert_to_firestore_document(&self, data: &Value) -> Value {
        if let Value::Object(obj) = data {
            let fields: serde_json::Map<String, Value> = obj
                .iter()
                .map(|(k, v)| (k.clone(), self.convert_to_firestore_value(v)))
                .collect();
            serde_json::json!({
                "fields": fields
            })
        } else {
            // If it's not an object, wrap it as a single field
            serde_json::json!({
                "fields": {
                    "data": self.convert_to_firestore_value(data)
                }
            })
        }
    }

    /// Convert JSON value to Firestore format
    fn convert_to_firestore_value(&self, value: &Value) -> Value {
        match value {
            Value::String(s) => serde_json::json!({"stringValue": s}),
            Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    serde_json::json!({"integerValue": i.to_string()})
                } else if let Some(f) = n.as_f64() {
                    serde_json::json!({"doubleValue": f})
                } else {
                    serde_json::json!({"stringValue": n.to_string()})
                }
            }
            Value::Bool(b) => serde_json::json!({"booleanValue": b}),
            Value::Array(arr) => serde_json::json!({
                "arrayValue": {
                    "values": arr.iter().map(|v| self.convert_to_firestore_value(v)).collect::<Vec<_>>()
                }
            }),
            Value::Object(obj) => {
                let fields: serde_json::Map<String, Value> = obj
                    .iter()
                    .map(|(k, v)| (k.clone(), self.convert_to_firestore_value(v)))
                    .collect();
                serde_json::json!({
                    "mapValue": {
                        "fields": fields
                    }
                })
            }
            Value::Null => serde_json::json!({"nullValue": null}),
        }
    }

    /// Convert from Firestore format to JSON value
    fn convert_from_firestore_value(&self, firestore_doc: &Value) -> Value {
        if let Some(fields) = firestore_doc.get("fields") {
            self.convert_firestore_fields_to_value(fields)
        } else {
            Value::Null
        }
    }

    fn convert_firestore_fields_to_value(&self, fields: &Value) -> Value {
        match fields {
            Value::Object(obj) => {
                let mut result = serde_json::Map::new();
                for (key, field_value) in obj {
                    result.insert(
                        key.clone(),
                        self.convert_single_firestore_value(field_value),
                    );
                }
                Value::Object(result)
            }
            _ => Value::Null,
        }
    }

    fn convert_single_firestore_value(&self, field_value: &Value) -> Value {
        if let Some(string_val) = field_value.get("stringValue") {
            string_val.clone()
        } else if let Some(int_val) = field_value.get("integerValue") {
            if let Some(s) = int_val.as_str() {
                if let Ok(n) = s.parse::<i64>() {
                    return Value::Number(serde_json::Number::from(n));
                }
            }
            int_val.clone()
        } else if let Some(bool_val) = field_value.get("booleanValue") {
            bool_val.clone()
        } else if let Some(array_val) = field_value.get("arrayValue") {
            if let Some(values) = array_val.get("values") {
                if let Value::Array(arr) = values {
                    let converted: Vec<Value> = arr
                        .iter()
                        .map(|v| self.convert_single_firestore_value(v))
                        .collect();
                    return Value::Array(converted);
                }
            }
            Value::Array(vec![])
        } else if let Some(map_val) = field_value.get("mapValue") {
            if let Some(fields) = map_val.get("fields") {
                return self.convert_firestore_fields_to_value(fields);
            }
            Value::Object(serde_json::Map::new())
        } else {
            Value::Null
        }
    }

    // Soundeo, Spotify, UrlList and Genre Tracker CRUD operations
    
    /// Get soundeo data from soundeo_tracks collection
    pub async fn get_soundeo(&self) -> Result<crate::soundeo::Soundeo, Box<dyn std::error::Error>> {
        // Get all tracks from soundeo_tracks collection and reconstruct Soundeo
        let tracks_map = self.get_all_soundeo_tracks().await?;
        let mut soundeo = crate::soundeo::Soundeo::new();
        soundeo.tracks_info = tracks_map;
        Ok(soundeo)
    }

    /// Get soundeo tracks info (HashMap only) - optimized method
    pub async fn get_soundeo_tracks_info(&self) -> Result<std::collections::HashMap<String, crate::soundeo::track::SoundeoTrack>, Box<dyn std::error::Error>> {
        self.get_all_soundeo_tracks().await
    }

    /// Get all soundeo tracks from Firebase
    pub async fn get_all_soundeo_tracks(&self) -> Result<std::collections::HashMap<String, crate::soundeo::track::SoundeoTrack>, Box<dyn std::error::Error>> {
        let mut tracks_map = std::collections::HashMap::new();
        let mut page_token: Option<String> = None;
        
        loop {
            let mut collection_url = format!(
                "{}/users/{}/soundeo_tracks?pageSize=1000",
                self.firestore_url(),
                urlencoding::encode(&self.user_id)
            );
            
            if let Some(token) = &page_token {
                collection_url.push_str(&format!("&pageToken={}", token));
            }
            
            let response = self.client.get(&collection_url).bearer_auth(&self.access_token).send().await?;
            
            if response.status() == StatusCode::NOT_FOUND {
                break;
            }
            
            let firestore_response: Value = response.json().await?;
            
            if let Some(documents) = firestore_response.get("documents") {
                if let Value::Array(docs) = documents {
                    for doc in docs {
                        if let Some(fields) = doc.get("fields") {
                            let track_value = self.convert_firestore_fields_to_value(fields);
                            if let Ok(track) = serde_json::from_value::<crate::soundeo::track::SoundeoTrack>(track_value) {
                                tracks_map.insert(track.id.clone(), track);
                            }
                        }
                    }
                }
            }
            
            // Check if there's a next page
            if let Some(next_page_token) = firestore_response.get("nextPageToken").and_then(|t| t.as_str()) {
                page_token = Some(next_page_token.to_string());
            } else {
                break; // No more pages
            }
        }
        
        Ok(tracks_map)
    }
    
    /// Save soundeo data - this is a no-op since we save individual tracks to soundeo_tracks collection
    pub async fn save_soundeo(&self, _soundeo: &crate::soundeo::Soundeo) -> Result<(), Box<dyn std::error::Error>> {
        // No-op: individual tracks are saved to soundeo_tracks collection via create_soundeo_track
        Ok(())
    }

    /// Get individual soundeo track from soundeo_tracks collection
    pub async fn get_soundeo_track(&self, track_id: &str) -> Result<Option<crate::soundeo::track::SoundeoTrack>, Box<dyn std::error::Error>> {
        let url = format!(
            "{}/users/{}/soundeo_tracks/{}",
            self.firestore_url(),
            urlencoding::encode(&self.user_id),
            urlencoding::encode(track_id)
        );
        
        let response = self.client.get(&url).bearer_auth(&self.access_token).send().await?;
        
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        
        let firestore_response: Value = response.json().await?;
        
        if let Some(fields) = firestore_response.get("fields") {
            let track_value = self.convert_firestore_fields_to_value(fields);
            let track: crate::soundeo::track::SoundeoTrack = serde_json::from_value(track_value)?;
            return Ok(Some(track));
        }
        
        Ok(None)
    }

    /// Save individual soundeo track to soundeo_tracks collection
    pub async fn save_soundeo_track(&self, track: &crate::soundeo::track::SoundeoTrack) -> Result<(), Box<dyn std::error::Error>> {
        let url = format!(
            "{}/users/{}/soundeo_tracks/{}",
            self.firestore_url(),
            urlencoding::encode(&self.user_id),
            urlencoding::encode(&track.id)
        );
        
        let track_value = serde_json::to_value(track)?;
        
        let firestore_doc = serde_json::json!({
            "fields": self.convert_to_firestore_value(&track_value).get("mapValue").unwrap().get("fields").unwrap()
        });
        
        let response = self.client
            .patch(&url)
            .bearer_auth(&self.access_token)
            .json(&firestore_doc)
            .send()
            .await?;
        
        if !response.status().is_success() {
            let error_text = response.text().await?;
            return Err(format!("Failed to save soundeo track: {}", error_text).into());
        }
        
        Ok(())
    }

    /// Get spotify data from dj_wizard_data
    pub async fn get_spotify(&self) -> Result<crate::spotify::Spotify, Box<dyn std::error::Error>> {
        let url = self.get_firebase_url(Self::COLLECTION_DJ_WIZARD_DATA);
        let response = self.client.get(&url).bearer_auth(&self.access_token).send().await?;
        
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(crate::spotify::Spotify::new());
        }
        
        let firestore_response: Value = response.json().await?;
        
        if let Some(fields) = firestore_response.get("fields") {
            if let Some(spotify_field) = fields.get("spotify") {
                if let Some(map_val) = spotify_field.get("mapValue") {
                    if let Some(spotify_fields) = map_val.get("fields") {
                        let spotify_value = self.convert_firestore_fields_to_value(spotify_fields);
                        return Ok(serde_json::from_value(spotify_value)?);
                    }
                }
            }
        }
        
        Ok(crate::spotify::Spotify::new())
    }
    
    /// Save spotify data to dj_wizard_data
    pub async fn save_spotify(&self, spotify: &crate::spotify::Spotify) -> Result<(), Box<dyn std::error::Error>> {
        let url = self.get_firebase_url(Self::COLLECTION_DJ_WIZARD_DATA);
        let spotify_value = serde_json::to_value(spotify)?;
        
        let firestore_doc = serde_json::json!({
            "fields": {
                "spotify": self.convert_to_firestore_value(&spotify_value)
            }
        });
        
        let response = self.client
            .patch(&url)
            .bearer_auth(&self.access_token)
            .query(&[("updateMask.fieldPaths", "spotify")])
            .json(&firestore_doc)
            .send()
            .await?;
        
        if !response.status().is_success() {
            let error_text = response.text().await?;
            return Err(format!("Failed to save spotify: {}", error_text).into());
        }
        
        Ok(())
    }

    /// Get url_list data from dj_wizard_data
    pub async fn get_url_list(&self) -> Result<std::collections::HashSet<String>, Box<dyn std::error::Error>> {
        let url = self.get_firebase_url(Self::COLLECTION_DJ_WIZARD_DATA);
        let response = self.client.get(&url).bearer_auth(&self.access_token).send().await?;
        
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(std::collections::HashSet::new());
        }
        
        let firestore_response: Value = response.json().await?;
        
        if let Some(fields) = firestore_response.get("fields") {
            if let Some(url_list_field) = fields.get("url_list") {
                if let Some(array_val) = url_list_field.get("arrayValue") {
                    if let Some(values) = array_val.get("values") {
                        if let Value::Array(arr) = values {
                            let mut url_set = std::collections::HashSet::new();
                            for item in arr {
                                if let Some(string_val) = item.get("stringValue") {
                                    if let Some(url) = string_val.as_str() {
                                        url_set.insert(url.to_string());
                                    }
                                }
                            }
                            return Ok(url_set);
                        }
                    }
                }
            }
        }
        
        Ok(std::collections::HashSet::new())
    }
    
    /// Save url_list data to dj_wizard_data
    pub async fn save_url_list(&self, url_list: &std::collections::HashSet<String>) -> Result<(), Box<dyn std::error::Error>> {
        let url = self.get_firebase_url(Self::COLLECTION_DJ_WIZARD_DATA);
        let url_vec: Vec<String> = url_list.iter().cloned().collect();
        let url_list_value = serde_json::to_value(url_vec)?;
        
        let firestore_doc = serde_json::json!({
            "fields": {
                "url_list": self.convert_to_firestore_value(&url_list_value)
            }
        });
        
        let response = self.client
            .patch(&url)
            .bearer_auth(&self.access_token)
            .query(&[("updateMask.fieldPaths", "url_list")])
            .json(&firestore_doc)
            .send()
            .await?;
        
        if !response.status().is_success() {
            let error_text = response.text().await?;
            return Err(format!("Failed to save url_list: {}", error_text).into());
        }
        
        Ok(())
    }

    /// Get all URLs from url_list collection with pagination
    pub async fn get_all_url_list_from_collection(&self) -> AuthResult<std::collections::HashSet<String>> {
        let mut url_set = std::collections::HashSet::new();
        let mut page_token: Option<String> = None;
        
        loop {
            let mut url = self.get_collection_path("url_list");
            url.push_str("?pageSize=1000"); // Request up to 1000 documents per page
            
            if let Some(token) = &page_token {
                url.push_str(&format!("&pageToken={}", token));
            }

            let response = self
                .client
                .get(&url)
                .bearer_auth(&self.access_token)
                .send()
                .await
                .map_err(|e| AuthError::new(&format!("Failed to get url_list collection: {}", e)))?;

            match response.status().as_u16() {
                200 => {
                    let firestore_response: serde_json::Value = response.json().await.map_err(|e| {
                        AuthError::new(&format!("Failed to parse url_list response: {}", e))
                    })?;

                    if let Some(documents) = firestore_response["documents"].as_array() {
                        for doc in documents {
                            if let Some(fields) = doc["fields"].as_object() {
                                if let Some(url_field) = fields.get("url") {
                                    if let Some(url_value) = url_field["stringValue"].as_str() {
                                        url_set.insert(url_value.to_string());
                                    }
                                }
                            }
                        }
                    }

                    // Check if there's a next page
                    if let Some(next_page_token) = firestore_response["nextPageToken"].as_str() {
                        page_token = Some(next_page_token.to_string());
                    } else {
                        break; // No more pages
                    }
                }
                404 => break, // No documents found
                _ => {
                    let error_text = response.text().await.unwrap_or("Unknown error".to_string());
                    return Err(
                        AuthError::new(&format!("Failed to get url_list collection: {}", error_text))
                            .into(),
                    );
                }
            }
        }

        Ok(url_set)
    }

    /// Get genre_tracker data from dj_wizard_data
    pub async fn get_genre_tracker(&self) -> Result<crate::genre_tracker::GenreTracker, Box<dyn std::error::Error>> {
        let url = self.get_firebase_url(Self::COLLECTION_DJ_WIZARD_DATA);
        let response = self.client.get(&url).bearer_auth(&self.access_token).send().await?;
        
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(crate::genre_tracker::GenreTracker::new());
        }
        
        let firestore_response: Value = response.json().await?;
        
        if let Some(fields) = firestore_response.get("fields") {
            if let Some(genre_tracker_field) = fields.get("genre_tracker") {
                if let Some(map_val) = genre_tracker_field.get("mapValue") {
                    if let Some(genre_tracker_fields) = map_val.get("fields") {
                        let genre_tracker_value = self.convert_firestore_fields_to_value(genre_tracker_fields);
                        return Ok(serde_json::from_value(genre_tracker_value)?);
                    }
                }
            }
        }
        
        Ok(crate::genre_tracker::GenreTracker::new())
    }
    
    /// Save genre_tracker data to dj_wizard_data
    pub async fn save_genre_tracker(&self, genre_tracker: &crate::genre_tracker::GenreTracker) -> Result<(), Box<dyn std::error::Error>> {
        let url = self.get_firebase_url(Self::COLLECTION_DJ_WIZARD_DATA);
        let genre_tracker_value = serde_json::to_value(genre_tracker)?;
        
        let firestore_doc = serde_json::json!({
            "fields": {
                "genre_tracker": self.convert_to_firestore_value(&genre_tracker_value)
            }
        });
        
        let response = self.client
            .patch(&url)
            .bearer_auth(&self.access_token)
            .query(&[("updateMask.fieldPaths", "genre_tracker")])
            .json(&firestore_doc)
            .send()
            .await?;
        
        if !response.status().is_success() {
            let error_text = response.text().await?;
            return Err(format!("Failed to save genre_tracker: {}", error_text).into());
        }
        
        Ok(())
    }

    // Artist CRUD operations

    /// Save artist manager to dj_wizard_data
    pub async fn save_artists(&self, artist_manager: &ArtistManager) -> AuthResult<()> {
        let data = serde_json::to_value(artist_manager)
            .map_err(|e| AuthError::new(&format!("Serialization error: {}", e)))?;

        // Save to dj_wizard_data document as artists field
        let url = self.get_firebase_url(Self::COLLECTION_DJ_WIZARD_DATA);

        let firestore_doc = serde_json::json!({
            "fields": {
                "artists": self.convert_to_firestore_value(&data)
            }
        });

        let response = self
            .client
            .patch(&url)
            .bearer_auth(&self.access_token)
            .query(&[("updateMask.fieldPaths", "artists")])
            .json(&firestore_doc)
            .send()
            .await
            .map_err(|e| AuthError::new(&format!("Failed to save artists data: {}", e)))?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or("Unknown error".to_string());
            return Err(
                AuthError::new(&format!("Failed to save artists data: {}", error_text)).into(),
            );
        }

        println!("✅ Artists data saved to dj_wizard_data successfully!");
        Ok(())
    }

    /// Load artist manager from Firestore
    pub async fn load_artists(&self) -> AuthResult<Option<ArtistManager>> {
        match self.get_document("artists", "favorite_artists").await? {
            Some(data) => {
                let artist_manager: ArtistManager = serde_json::from_value(data)
                    .map_err(|e| AuthError::new(&format!("Deserialization error: {}", e)))?;
                Ok(Some(artist_manager))
            }
            None => Ok(None),
        }
    }

    /// Add or update a specific tracked genre
    pub async fn save_tracked_genre(
        &self,
        genre_id: u32,
        tracked_genre: &TrackedGenre,
    ) -> AuthResult<()> {
        let data = serde_json::to_value(tracked_genre)
            .map_err(|e| AuthError::new(&format!("Serialization error: {}", e)))?;

        let collection = format!("genre_tracker/main/tracked_genres");
        self.set_document(&collection, &genre_id.to_string(), &data)
            .await
    }

    /// Get a specific tracked genre
    pub async fn get_tracked_genre(&self, genre_id: u32) -> AuthResult<Option<TrackedGenre>> {
        let collection = format!("genre_tracker/main/tracked_genres");
        match self
            .get_document(&collection, &genre_id.to_string())
            .await?
        {
            Some(data) => {
                let tracked_genre: TrackedGenre = serde_json::from_value(data)
                    .map_err(|e| AuthError::new(&format!("Deserialization error: {}", e)))?;
                Ok(Some(tracked_genre))
            }
            None => Ok(None),
        }
    }

    /// Delete a specific tracked genre
    pub async fn delete_tracked_genre(&self, genre_id: u32) -> AuthResult<()> {
        let collection = format!("genre_tracker/main/tracked_genres");
        let url = format!(
            "{}/{}/{}",
            self.firestore_url(),
            self.user_collection_path(&collection),
            genre_id.to_string()
        );

        let response = self
            .client
            .delete(&url)
            .header("Authorization", format!("Bearer {}", self.access_token))
            .send()
            .await
            .map_err(|e| AuthError::new(&format!("Failed to delete tracked genre: {}", e)))?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(AuthError::new(&format!("Firestore delete error: {}", error_text)).into());
        }

        Ok(())
    }

    /// Add or update available genre info
    pub async fn save_genre_info(&self, genre_id: u32, genre_info: &GenreInfo) -> AuthResult<()> {
        let data = serde_json::to_value(genre_info)
            .map_err(|e| AuthError::new(&format!("Serialization error: {}", e)))?;

        let collection = format!("genre_tracker/main/available_genres");
        self.set_document(&collection, &genre_id.to_string(), &data)
            .await
    }

    /// Get available genre info
    pub async fn get_genre_info(&self, genre_id: u32) -> AuthResult<Option<GenreInfo>> {
        let collection = format!("genre_tracker/main/available_genres");
        match self
            .get_document(&collection, &genre_id.to_string())
            .await?
        {
            Some(data) => {
                let genre_info: GenreInfo = serde_json::from_value(data)
                    .map_err(|e| AuthError::new(&format!("Deserialization error: {}", e)))?;
                Ok(Some(genre_info))
            }
            None => Ok(None),
        }
    }

    // Migration utilities

    /// Migrate local artist data to Firebase
    pub async fn migrate_artists_from_local(&self, local_path: &str) -> AuthResult<()> {
        // Read local artist data
        let local_data = std::fs::read_to_string(local_path)
            .map_err(|e| AuthError::new(&format!("Failed to read local artist file: {}", e)))?;

        let artist_manager: ArtistManager = serde_json::from_str(&local_data)
            .map_err(|e| AuthError::new(&format!("Failed to parse local artist data: {}", e)))?;

        // Save to Firebase
        self.save_artists(&artist_manager).await?;

        println!(
            "✅ Successfully migrated {} artists to Firebase",
            artist_manager.favorite_artists.len()
        );
        Ok(())
    }

    /// Migrate local genre tracker data to Firebase
    pub async fn migrate_genre_tracker_from_local(&self, local_path: &str) -> AuthResult<()> {
        // Read local genre tracker data
        let local_data = std::fs::read_to_string(local_path).map_err(|e| {
            AuthError::new(&format!("Failed to read local genre tracker file: {}", e))
        })?;

        let genre_tracker: GenreTracker = serde_json::from_str(&local_data).map_err(|e| {
            AuthError::new(&format!("Failed to parse local genre tracker data: {}", e))
        })?;

        // Save to Firebase
        self.save_genre_tracker(&genre_tracker).await.map_err(|e| AuthError::new(&format!("Failed to save genre tracker: {}", e)))?;

        println!(
            "✅ Successfully migrated {} tracked genres and {} available genres to Firebase",
            genre_tracker.tracked_genres.len(),
            genre_tracker.available_genres.len()
        );
        Ok(())
    }

    /// Full migration from local data files
    pub async fn migrate_all_from_local(
        &self,
        artists_path: &str,
        genre_tracker_path: &str,
    ) -> AuthResult<()> {
        println!("🔄 Starting migration from local files to Firebase...");

        // Migrate artists if file exists
        if std::path::Path::new(artists_path).exists() {
            println!("📂 Migrating artists from: {}", artists_path);
            self.migrate_artists_from_local(artists_path).await?;
        } else {
            println!("⚠️  Artists file not found: {}", artists_path);
        }

        // Migrate genre tracker if file exists
        if std::path::Path::new(genre_tracker_path).exists() {
            println!("📂 Migrating genre tracker from: {}", genre_tracker_path);
            self.migrate_genre_tracker_from_local(genre_tracker_path)
                .await?;
        } else {
            println!("⚠️  Genre tracker file not found: {}", genre_tracker_path);
        }

        println!("🎉 Migration completed successfully!");
        Ok(())
    }

    // Individual Track CRUD operations for O(1) access

    /// Save a single track to Firebase (O(1) access by ID)
    pub async fn save_track(
        &self,
        track_id: &str,
        track: &crate::soundeo::track::SoundeoTrack,
    ) -> AuthResult<()> {
        let data = serde_json::to_value(track)
            .map_err(|e| AuthError::new(&format!("Serialization error: {}", e)))?;

        // Use simple flat collection structure for O(1) access
        self.set_document("soundeo_tracks", track_id, &data).await
    }

    /// Get a single track from Firebase by ID (O(1) access)
    pub async fn get_track(
        &self,
        track_id: &str,
    ) -> AuthResult<Option<crate::soundeo::track::SoundeoTrack>> {
        match self.get_document("soundeo_tracks", track_id).await? {
            Some(data) => {
                let track: crate::soundeo::track::SoundeoTrack = serde_json::from_value(data)
                    .map_err(|e| AuthError::new(&format!("Deserialization error: {}", e)))?;
                Ok(Some(track))
            }
            None => Ok(None),
        }
    }

    /// Get multiple tracks from Firebase by IDs (O(n) batch access)
    pub async fn get_tracks(
        &self,
        track_ids: &[String],
    ) -> AuthResult<std::collections::HashMap<String, crate::soundeo::track::SoundeoTrack>> {
        use futures_util::stream::{FuturesUnordered, StreamExt};
        use std::sync::Arc;

        let client = Arc::new(self);
        let mut futures = FuturesUnordered::new();

        // Create concurrent requests for all track IDs
        for track_id in track_ids {
            let client_clone = client.clone();
            let id = track_id.clone();

            let future = async move {
                let result = client_clone.get_track(&id).await;
                (id, result)
            };
            futures.push(future);
        }

        let mut tracks = std::collections::HashMap::new();

        // Collect all results
        while let Some((track_id, result)) = futures.next().await {
            match result {
                Ok(Some(track)) => {
                    tracks.insert(track_id, track);
                }
                Ok(None) => {
                    // Track not found, skip
                }
                Err(e) => {
                    // Log error but continue with other tracks
                    eprintln!("⚠️  Failed to get track {}: {}", track_id, e);
                }
            }
        }

        Ok(tracks)
    }

    /// Delete a single track from Firebase by ID (O(1) access)
    pub async fn delete_track(&self, track_id: &str) -> AuthResult<()> {
        let url = format!(
            "{}/{}",
            self.firestore_url(),
            self.user_collection_path("soundeo_tracks")
        );
        let full_url = format!("{}/{}", url, track_id);

        let response = self
            .client
            .delete(&full_url)
            .header("Authorization", format!("Bearer {}", self.access_token))
            .send()
            .await
            .map_err(|e| AuthError::new(&format!("Failed to delete track: {}", e)))?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(AuthError::new(&format!("Firestore delete error: {}", error_text)).into());
        }

        Ok(())
    }

    /// Check if a track exists by ID (O(1) access)
    pub async fn track_exists(&self, track_id: &str) -> AuthResult<bool> {
        match self.get_track(track_id).await? {
            Some(_) => Ok(true),
            None => Ok(false),
        }
    }

    /// Batch write multiple tracks (up to 500 per batch for optimal performance)
    pub async fn batch_write_tracks(
        &self,
        tracks: &[(String, crate::soundeo::track::SoundeoTrack)],
    ) -> AuthResult<()> {
        // Note: For batch operations, we don't check token expiry as they are typically quick

        if tracks.is_empty() {
            return Ok(());
        }

        // Firebase batch write limit is 500 operations
        if tracks.len() > 500 {
            return Err(
                AuthError::new("Batch size exceeds Firebase limit of 500 operations").into(),
            );
        }

        let user_id = &self.user_id;
        let batch_url = format!(
            "https://firestore.googleapis.com/v1/projects/{}/databases/(default)/documents:batchWrite",
            self.project_id
        );

        // Build batch writes
        let mut writes = Vec::new();
        for (track_id, track) in tracks {
            let document_path = format!(
                "projects/{}/databases/(default)/documents/users/{}/soundeo_tracks/{}",
                self.project_id, user_id, track_id
            );

            // Only include essential fields for Firebase
            let mut fields = serde_json::Map::new();
            fields.insert(
                "id".to_string(),
                self.convert_to_firestore_value(&serde_json::Value::String(track.id.clone())),
            );
            fields.insert(
                "title".to_string(),
                self.convert_to_firestore_value(&serde_json::Value::String(track.title.clone())),
            );
            fields.insert(
                "track_url".to_string(),
                self.convert_to_firestore_value(&serde_json::Value::String(
                    track.track_url.clone(),
                )),
            );
            fields.insert(
                "date".to_string(),
                self.convert_to_firestore_value(&serde_json::Value::String(track.date.clone())),
            );
            fields.insert(
                "genre".to_string(),
                self.convert_to_firestore_value(&serde_json::Value::String(track.genre.clone())),
            );
            fields.insert(
                "downloadable".to_string(),
                self.convert_to_firestore_value(&serde_json::Value::Bool(track.downloadable)),
            );
            fields.insert(
                "already_downloaded".to_string(),
                self.convert_to_firestore_value(&serde_json::Value::Bool(track.already_downloaded)),
            );

            writes.push(serde_json::json!({
                "update": {
                    "name": document_path,
                    "fields": fields
                }
            }));
        }

        let batch_request = serde_json::json!({
            "writes": writes
        });

        let response = self
            .client
            .post(&batch_url)
            .bearer_auth(&self.access_token)
            .json(&batch_request)
            .send()
            .await
            .map_err(|e| AuthError::new(&format!("HTTP request failed: {}", e)))?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(
                AuthError::new(&format!("Firestore batch write error: {}", error_text)).into(),
            );
        }

        // Mark all tracks in the batch as migrated (save is automatic)
        for (track_id, _) in tracks {
            let _ = crate::log::DjWizardLog::mark_track_as_migrated(track_id);
        }

        Ok(())
    }

    /// Migrate queue tracks to priority-based structure using batch processing
    pub async fn migrate_queue_to_subcollections(
        &mut self,
        queued_tracks: &[crate::log::QueuedTrack],
    ) -> AuthResult<()> {
        self.ensure_valid_token().await?;

        // STEP 1: Get existing queued tracks from Firebase and mark them as migrated locally
        println!("🔍 Checking existing queued tracks in Firebase...");
        let existing_ids = self.get_all_firebase_queue_ids().await.unwrap_or_default();
        println!(
            "📊 Found {} existing queued tracks in Firebase",
            existing_ids.len()
        );

        if !existing_ids.is_empty() {
            println!(
                "📝 Storing {} existing queued track IDs in bulk locally...",
                existing_ids.len()
            );
            let _ = crate::log::DjWizardLog::set_firebase_migrated_queues(existing_ids.clone());
            println!("✅ Stored existing queued track IDs in bulk (single save operation)");
        }

        // STEP 2: Filter only tracks NOT in Firebase
        let tracks_to_migrate: Vec<&crate::log::QueuedTrack> = queued_tracks
            .iter()
            .filter(|track| !existing_ids.contains(&track.track_id))
            .collect();

        if tracks_to_migrate.is_empty() {
            println!("✅ All queued tracks already exist in Firebase");
            return Ok(());
        }

        println!(
            "📋 Will migrate {} new queued tracks to Firebase",
            tracks_to_migrate.len()
        );

        let total = tracks_to_migrate.len();
        let start_time = std::time::Instant::now();
        let completed = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let failed = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let processed = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));

        let concurrent_batches = 2;

        // Initialize status display - statistics line + worker lines
        println!(
            "📋 Queue: 0/{} | ✅ 0 ❌ 0 | ⏱️ 0s | 🚀 0.0/min | ⏳ ∞",
            total
        );
        for i in 0..concurrent_batches {
            println!("Batch Thread {}: Waiting...", i);
        }

        // Share start time for rate calculation
        let start_time = std::sync::Arc::new(start_time);

        // Convert to shared queue
        let tracks_to_migrate_owned: Vec<crate::log::QueuedTrack> =
            tracks_to_migrate.into_iter().cloned().collect();
        let tracks_queue = std::sync::Arc::new(tokio::sync::Mutex::new(tracks_to_migrate_owned));

        // Spawn 2 worker threads
        let mut tasks = Vec::new();
        for thread_id in 0..concurrent_batches {
            let tracks_queue = tracks_queue.clone();
            let client = self.client.clone();
            let access_token = self.access_token.clone();
            let user_id = self.user_id.clone();
            let firestore_url = self.firestore_url();
            let completed_clone = completed.clone();
            let failed_clone = failed.clone();
            let processed_clone = processed.clone();
            let start_time_clone = start_time.clone();

            let task = tokio::spawn(async move {
                // Small delay to stagger worker startup
                if thread_id > 0 {
                    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
                }
                Self::process_queue_worker(
                    tracks_queue,
                    client,
                    access_token,
                    user_id,
                    firestore_url,
                    completed_clone,
                    failed_clone,
                    processed_clone,
                    thread_id,
                    start_time_clone,
                    total,
                )
                .await
            });

            tasks.push(task);
        }

        // Wait for all workers to complete
        for task in tasks {
            let _ = task.await;
        }

        // Check token periodically during processing
        self.ensure_valid_token().await?;

        // Move cursor to statistics line for final update
        print!("\x1B[{}A", concurrent_batches);
        let final_elapsed = start_time.elapsed();
        let final_completed = completed.load(std::sync::atomic::Ordering::Relaxed);
        let final_failed = failed.load(std::sync::atomic::Ordering::Relaxed);
        let final_rate = if final_elapsed.as_secs() > 0 {
            (final_completed as f64 / final_elapsed.as_secs_f64()) * 60.0
        } else {
            0.0
        };

        print!(
            "\x1B[2K\r📋 Queue: {}/{} | ✅ {} ❌ {} | ⏱️ {:?} | 🚀 {:.1}/min | ✅ Complete!",
            total, total, final_completed, final_failed, final_elapsed, final_rate
        );
        print!("\x1B[{}B", concurrent_batches);
        println!();

        if final_failed > 0 {
            return Err(AuthError::new(&format!(
                "Migration completed with {} failures",
                final_failed
            ))
            .into());
        }

        Ok(())
    }

    async fn process_queue_worker(
        tracks_queue: std::sync::Arc<tokio::sync::Mutex<Vec<crate::log::QueuedTrack>>>,
        client: reqwest::Client,
        access_token: String,
        user_id: String,
        firestore_url: String,
        completed: std::sync::Arc<std::sync::atomic::AtomicUsize>,
        failed: std::sync::Arc<std::sync::atomic::AtomicUsize>,
        processed: std::sync::Arc<std::sync::atomic::AtomicUsize>,
        thread_id: usize,
        start_time: std::sync::Arc<std::time::Instant>,
        total: usize,
    ) {
        let concurrent_batches = 2;

        loop {
            // Get next track from queue (quick lock/unlock)
            let track = {
                let mut queue = tracks_queue.lock().await;
                queue.pop()
            };

            let Some(track) = track else {
                // No more tracks, show waiting
                print!("\x1B[{}A", concurrent_batches - thread_id);
                print!("\x1B[2K\rBatch Thread {}: Waiting...", thread_id);
                print!("\x1B[{}B", concurrent_batches - thread_id);
                std::io::Write::flush(&mut std::io::stdout()).unwrap_or_default();
                break;
            };

            // Show processing this track
            print!("\x1B[{}A", concurrent_batches - thread_id);
            print!(
                "\x1B[2K\rBatch Thread {}: Processing {}",
                thread_id, track.track_id
            );
            print!("\x1B[{}B", concurrent_batches - thread_id);
            std::io::Write::flush(&mut std::io::stdout()).unwrap_or_default();

            let priority_name = match track.priority {
                crate::log::Priority::High => "high",
                crate::log::Priority::Normal => "normal",
                crate::log::Priority::Low => "low",
            };

            let url = format!(
                "{}/users/{}/queued_tracks/{}",
                firestore_url, user_id, track.track_id
            );

            let result = client
                .patch(&url)
                .bearer_auth(&access_token)
                .query(&[
                    ("updateMask.fieldPaths", "track_id"),
                    ("updateMask.fieldPaths", "order_key"),
                    ("updateMask.fieldPaths", "priority"),
                ])
                .json(&serde_json::json!({
                    "fields": {
                        "track_id": {"stringValue": track.track_id},
                        "order_key": {"doubleValue": track.order_key},
                        "priority": {"stringValue": priority_name}
                    }
                }))
                .send()
                .await;

            match result {
                Ok(response) if response.status().is_success() => {
                    completed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let _ = crate::log::DjWizardLog::mark_queued_track_as_migrated(&track.track_id);
                }
                Ok(_) | Err(_) => {
                    failed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
            }

            processed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

            // Update statistics after each track
            let elapsed = start_time.elapsed();
            let current_processed = processed.load(std::sync::atomic::Ordering::Relaxed);
            let current_completed = completed.load(std::sync::atomic::Ordering::Relaxed);
            let current_failed = failed.load(std::sync::atomic::Ordering::Relaxed);

            let rate = if elapsed.as_secs() > 0 {
                (current_completed as f64 / elapsed.as_secs_f64()) * 60.0
            } else {
                0.0
            };

            let eta = if rate > 0.0 {
                let remaining = total - current_processed;
                let minutes_remaining = remaining as f64 / rate;
                if minutes_remaining < 60.0 {
                    format!("{:.0}m", minutes_remaining)
                } else {
                    format!("{:.1}h", minutes_remaining / 60.0)
                }
            } else {
                "∞".to_string()
            };

            // Update statistics line (move up 2 lines, update, move back down)
            print!("\x1B[{}A", concurrent_batches);
            print!(
                "\x1B[2K\r📋 Queue: {}/{} | ✅ {} ❌ {} | ⏱️ {:?} | 🚀 {:.1}/min | ⏳ {}",
                current_processed, total, current_completed, current_failed, elapsed, rate, eta
            );
            print!("\x1B[{}B", concurrent_batches);
            std::io::Write::flush(&mut std::io::stdout()).unwrap_or_default();
        }
    }

    /// Get all track IDs from Firebase with pagination
    pub async fn get_all_firebase_track_ids(&self) -> AuthResult<Vec<String>> {
        let mut track_ids = Vec::new();
        let mut page_token: Option<String> = None;

        loop {
            let mut url = format!(
                "{}/users/{}/soundeo_tracks?pageSize=1000",
                self.firestore_url(),
                self.user_id
            );

            if let Some(token) = &page_token {
                url.push_str(&format!("&pageToken={}", token));
            }

            let response = self
                .client
                .get(&url)
                .bearer_auth(&self.access_token)
                .send()
                .await
                .map_err(|e| AuthError::new(&format!("HTTP request failed: {}", e)))?;

            if !response.status().is_success() {
                let error_text = response.text().await.unwrap_or_default();
                return Err(AuthError::new(&format!("Firebase get error: {}", error_text)).into());
            }

            let response_text = response
                .text()
                .await
                .map_err(|e| AuthError::new(&format!("Failed to read response: {}", e)))?;

            let json: serde_json::Value = serde_json::from_str(&response_text)
                .map_err(|e| AuthError::new(&format!("Failed to parse JSON: {}", e)))?;

            // Process documents from this page
            if let Some(documents) = json.get("documents").and_then(|d| d.as_array()) {
                for doc in documents {
                    if let Some(name) = doc.get("name").and_then(|n| n.as_str()) {
                        // Extract track ID from document path like "projects/.../documents/users/USER/soundeo_tracks/TRACK_ID"
                        if let Some(track_id) = name.split('/').last() {
                            track_ids.push(track_id.to_string());
                        }
                    }
                }
            }

            // Check for next page token
            page_token = json
                .get("nextPageToken")
                .and_then(|t| t.as_str())
                .map(|s| s.to_string());

            // If no next page token, we're done
            if page_token.is_none() {
                break;
            }
        }

        Ok(track_ids)
    }

    /// Get all queued track IDs from Firebase with pagination
    pub async fn get_all_firebase_queue_ids(&self) -> AuthResult<Vec<String>> {
        let mut track_ids = Vec::new();
        let mut page_token: Option<String> = None;

        loop {
            let mut url = format!(
                "{}/users/{}/queued_tracks?pageSize=1000",
                self.firestore_url(),
                self.user_id
            );

            if let Some(token) = &page_token {
                url.push_str(&format!("&pageToken={}", token));
            }

            let response = self
                .client
                .get(&url)
                .bearer_auth(&self.access_token)
                .send()
                .await
                .map_err(|e| AuthError::new(&format!("HTTP request failed: {}", e)))?;

            if !response.status().is_success() {
                let error_text = response.text().await.unwrap_or_default();
                return Err(AuthError::new(&format!("Firebase get error: {}", error_text)).into());
            }

            let response_text = response
                .text()
                .await
                .map_err(|e| AuthError::new(&format!("Failed to read response: {}", e)))?;

            let json: serde_json::Value = serde_json::from_str(&response_text)
                .map_err(|e| AuthError::new(&format!("Failed to parse JSON: {}", e)))?;

            // Process documents from this page
            if let Some(documents) = json.get("documents").and_then(|d| d.as_array()) {
                for doc in documents {
                    if let Some(name) = doc.get("name").and_then(|n| n.as_str()) {
                        // Extract track ID from document path
                        if let Some(track_id) = name.split('/').last() {
                            track_ids.push(track_id.to_string());
                        }
                    }
                }
            }

            // Check for next page token
            page_token = json
                .get("nextPageToken")
                .and_then(|t| t.as_str())
                .map(|s| s.to_string());

            // If no next page token, we're done
            if page_token.is_none() {
                break;
            }
        }

        Ok(track_ids)
    }

    // Queue CRUD operations for day-to-day queue management

    /// Get all queued tracks from Firebase
    pub async fn get_queued_tracks(&self) -> AuthResult<Vec<crate::log::QueuedTrack>> {
        let mut queued_tracks = Vec::new();
        let mut next_page_token = None;

        loop {
            let mut url = format!(
                "{}/users/{}/queued_tracks?pageSize=1000",
                self.firestore_url(),
                urlencoding::encode(&self.user_id)
            );

            if let Some(token) = &next_page_token {
                url = format!("{}&pageToken={}", url, token);
            }

            let response = self
                .client
                .get(&url)
                .bearer_auth(&self.access_token)
                .send()
                .await
                .map_err(|e| AuthError::new(&format!("Failed to get queued tracks: {}", e)))?;

            if !response.status().is_success() {
                return Err(AuthError::new(&format!(
                    "Firebase returned error: {}",
                    response.status()
                ))
                .into());
            }

            let data: serde_json::Value = response
                .json()
                .await
                .map_err(|e| AuthError::new(&format!("Failed to parse response: {}", e)))?;

            if let Some(documents) = data["documents"].as_array() {
                for doc in documents {
                    if let Some(fields) = doc["fields"].as_object() {
                        if let (Some(track_id), Some(priority), Some(order_key)) = (
                            fields
                                .get("track_id")
                                .and_then(|f| f["stringValue"].as_str()),
                            fields
                                .get("priority")
                                .and_then(|f| f["stringValue"].as_str()),
                            fields
                                .get("order_key")
                                .and_then(|f| f["doubleValue"].as_f64()),
                        ) {
                            let priority = match priority {
                                "High" => crate::log::Priority::High,
                                "Normal" => crate::log::Priority::Normal,
                                "Low" => crate::log::Priority::Low,
                                _ => crate::log::Priority::Normal,
                            };

                            // Use order_key as fallback for added_at since it's missing in Firebase
                            let added_at = fields
                                .get("added_at")
                                .and_then(|f| f["integerValue"].as_str())
                                .and_then(|s| s.parse::<u64>().ok())
                                .unwrap_or(order_key as u64);

                            queued_tracks.push(crate::log::QueuedTrack {
                                track_id: track_id.to_string(),
                                priority,
                                order_key,
                                added_at,
                                migrated: true,
                            });
                        }
                    }
                }
            }

            next_page_token = data["nextPageToken"].as_str().map(|s| s.to_string());
            if next_page_token.is_none() {
                break;
            }
        }

        Ok(queued_tracks)
    }

    /// Add a single track to Firebase queue
    pub async fn add_queued_track(
        &self,
        track_id: &str,
        priority: crate::log::Priority,
    ) -> AuthResult<bool> {
        // Check if track already exists
        if self.queued_track_exists(track_id).await? {
            return Ok(false); // Already exists
        }

        let order_key = chrono::Utc::now().timestamp_millis() as f64;
        let added_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| AuthError::new(&format!("Time error: {}", e)))?
            .as_secs();

        let priority_str = match priority {
            crate::log::Priority::High => "High",
            crate::log::Priority::Normal => "Normal",
            crate::log::Priority::Low => "Low",
        };

        let document_data = serde_json::json!({
            "fields": {
                "track_id": {"stringValue": track_id},
                "priority": {"stringValue": priority_str},
                "order_key": {"doubleValue": order_key},
                "added_at": {"integerValue": added_at.to_string()}
            }
        });

        let url = format!(
            "{}/users/{}/queued_tracks/{}",
            self.firestore_url(),
            urlencoding::encode(&self.user_id),
            track_id
        );

        let response = self
            .client
            .patch(&url)
            .bearer_auth(&self.access_token)
            .json(&document_data)
            .send()
            .await
            .map_err(|e| AuthError::new(&format!("Failed to add queued track: {}", e)))?;

        if response.status().is_success() {
            Ok(true) // Added successfully
        } else {
            Err(AuthError::new(&format!(
                "Failed to add queued track: {}",
                response.status()
            ))
            .into())
        }
    }

    /// Remove a track from Firebase queue
    pub async fn remove_queued_track(&self, track_id: &str) -> AuthResult<bool> {
        let url = format!(
            "{}/users/{}/queued_tracks/{}",
            self.firestore_url(),
            urlencoding::encode(&self.user_id),
            track_id
        );

        let response = self
            .client
            .delete(&url)
            .bearer_auth(&self.access_token)
            .send()
            .await
            .map_err(|e| AuthError::new(&format!("Failed to remove queued track: {}", e)))?;

        match response.status().as_u16() {
            200 => Ok(true),  // Successfully deleted
            404 => Ok(false), // Track wasn't in queue anyway
            _ => Err(AuthError::new(&format!(
                "Failed to remove queued track: {}",
                response.status()
            ))
            .into()),
        }
    }

    /// Check if a track exists in Firebase queue
    pub async fn queued_track_exists(&self, track_id: &str) -> AuthResult<bool> {
        let url = format!(
            "{}/users/{}/queued_tracks/{}",
            self.firestore_url(),
            urlencoding::encode(&self.user_id),
            track_id
        );

        let response = self
            .client
            .get(&url)
            .bearer_auth(&self.access_token)
            .send()
            .await
            .map_err(|e| AuthError::new(&format!("Failed to check queued track: {}", e)))?;

        Ok(response.status().is_success())
    }

    /// Update priority of an existing queued track
    pub async fn update_queued_track_priority(
        &self,
        track_id: &str,
        new_priority: crate::log::Priority,
    ) -> AuthResult<bool> {
        // Check if track exists first
        if !self.queued_track_exists(track_id).await? {
            return Ok(false); // Track not in queue
        }

        let priority_str = match new_priority {
            crate::log::Priority::High => "High",
            crate::log::Priority::Normal => "Normal",
            crate::log::Priority::Low => "Low",
        };

        let update_data = serde_json::json!({
            "fields": {
                "priority": {"stringValue": priority_str}
            }
        });

        let url = format!(
            "{}/users/{}/queued_tracks/{}",
            self.firestore_url(),
            urlencoding::encode(&self.user_id),
            track_id
        );

        let response = self
            .client
            .patch(&url)
            .bearer_auth(&self.access_token)
            .query(&[("updateMask.fieldPaths", "priority")])
            .json(&update_data)
            .send()
            .await
            .map_err(|e| {
                AuthError::new(&format!("Failed to update queued track priority: {}", e))
            })?;

        if response.status().is_success() {
            Ok(true) // Updated successfully
        } else {
            Err(AuthError::new(&format!(
                "Failed to update queued track priority: {}",
                response.status()
            ))
            .into())
        }
    }

    // Additional collections migration methods for light_only

    /// Save available_tracks collection to Firebase using batch write
    pub async fn save_available_tracks(
        &self,
        available_tracks: &std::collections::HashSet<String>,
    ) -> AuthResult<()> {
        let total = available_tracks.len();
        println!(
            "📋 Migrating {} available tracks to Firebase using batch processing...",
            total
        );

        if available_tracks.is_empty() {
            println!("✅ No available tracks to migrate.");
            return Ok(());
        }

        // Convert to batch format (max 500 per batch)
        let batch_size = 20;
        let tracks: Vec<&String> = available_tracks.iter().collect();
        let chunks: Vec<&[&String]> = tracks.chunks(batch_size).collect();

        for (i, chunk) in chunks.iter().enumerate() {
            println!(
                "📦 Processing batch {}/{} ({} tracks)...",
                i + 1,
                chunks.len(),
                chunk.len()
            );

            let mut writes = Vec::new();

            for track_id in *chunk {
                let document_name = format!(
                    "projects/{}/databases/(default)/documents/users/{}/available_tracks/{}",
                    self.project_id, self.user_id, track_id
                );

                let mut fields = std::collections::HashMap::new();
                fields.insert(
                    "track_id".to_string(),
                    self.convert_to_firestore_value(&serde_json::Value::String(
                        track_id.to_string(),
                    )),
                );
                fields.insert(
                    "added_at".to_string(),
                    self.convert_to_firestore_value(&serde_json::Value::Number(
                        serde_json::Number::from(chrono::Utc::now().timestamp()),
                    )),
                );

                writes.push(serde_json::json!({
                    "update": {
                        "name": document_name,
                        "fields": fields
                    }
                }));
            }

            // Send batch request
            let batch_data = serde_json::json!({
                "writes": writes
            });

            let batch_url = format!(
                "https://firestore.googleapis.com/v1/projects/{}/databases/(default)/documents:batchWrite",
                self.project_id
            );

            let response = self
                .client
                .post(&batch_url)
                .bearer_auth(&self.access_token)
                .json(&batch_data)
                .send()
                .await
                .map_err(|e| AuthError::new(&format!("Batch request failed: {}", e)))?;

            if !response.status().is_success() {
                let error_text = response.text().await.unwrap_or("Unknown error".to_string());
                return Err(AuthError::new(&format!("Batch write failed: {}", error_text)).into());
            }

            let processed = (i + 1) * batch_size.min(chunk.len());
            println!(
                "✅ Processed {}/{} available tracks",
                processed.min(total),
                total
            );
        }

        println!("✅ All {} available tracks migrated successfully!", total);
        Ok(())
    }

    /// Save spotify collection to Firebase
    pub async fn save_spotify_collection(
        &self,
        spotify: &crate::spotify::Spotify,
    ) -> AuthResult<()> {
        let data = serde_json::to_value(spotify)
            .map_err(|e| AuthError::new(&format!("Serialization error: {}", e)))?;

        // Save to dj_wizard_data document as spotify field
        let url = self.get_firebase_url(Self::COLLECTION_DJ_WIZARD_DATA);

        let firestore_doc = serde_json::json!({
            "fields": {
                "spotify": self.convert_to_firestore_value(&data)
            }
        });

        let response = self
            .client
            .patch(&url)
            .bearer_auth(&self.access_token)
            .query(&[("updateMask.fieldPaths", "spotify")])
            .json(&firestore_doc)
            .send()
            .await
            .map_err(|e| AuthError::new(&format!("Failed to save spotify data: {}", e)))?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or("Unknown error".to_string());
            return Err(
                AuthError::new(&format!("Failed to save spotify data: {}", error_text)).into(),
            );
        }

        println!("✅ Spotify data saved to dj_wizard_data successfully!");
        Ok(())
    }

    /// Save url_list collection to Firebase (old method for migration)
    pub async fn migrate_save_url_list(
        &self,
        url_list: &std::collections::HashSet<String>,
    ) -> AuthResult<()> {
        println!("📋 Migrating {} URLs to Firebase...", url_list.len());

        for url in url_list {
            let document_data = serde_json::json!({
                "fields": {
                    "url": {"stringValue": url},
                    "added_at": {"integerValue": chrono::Utc::now().timestamp().to_string()}
                }
            });

            // Use URL hash as document ID to avoid issues with special characters
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};
            let mut hasher = DefaultHasher::new();
            url.hash(&mut hasher);
            let url_hash = format!("{:x}", hasher.finish());
            let doc_url = format!(
                "{}/users/{}/url_list/{}",
                self.firestore_url(),
                urlencoding::encode(&self.user_id),
                url_hash
            );

            let response = self
                .client
                .patch(&doc_url)
                .bearer_auth(&self.access_token)
                .json(&document_data)
                .send()
                .await
                .map_err(|e| AuthError::new(&format!("Failed to save URL: {}", e)))?;

            if !response.status().is_success() {
                return Err(
                    AuthError::new(&format!("Failed to save URL: {}", response.status())).into(),
                );
            }
        }

        println!("✅ URL list migrated successfully!");
        Ok(())
    }

    /// Create or initialize the dj_wizard_data document
    pub async fn create_dj_wizard_data_document(&self) -> AuthResult<()> {
        let url = self.get_firebase_url(Self::COLLECTION_DJ_WIZARD_DATA);

        // Create empty document structure
        let empty_doc = serde_json::json!({
            "fields": {}
        });

        let response = self
            .client
            .patch(&url)
            .bearer_auth(&self.access_token)
            .json(&empty_doc)
            .send()
            .await
            .map_err(|e| {
                AuthError::new(&format!("Failed to create dj_wizard_data document: {}", e))
            })?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or("Unknown error".to_string());
            return Err(AuthError::new(&format!(
                "Failed to create dj_wizard_data document: {}",
                error_text
            ))
            .into());
        }

        println!("✅ dj_wizard_data document created successfully!");
        Ok(())
    }

    /// Get available tracks from Firebase
    pub async fn get_available_tracks(&self) -> AuthResult<std::collections::HashSet<String>> {
        let mut available_tracks = std::collections::HashSet::new();
        let mut page_token: Option<String> = None;
        
        loop {
            let mut url = self.get_collection_path("available_tracks");
            url.push_str("?pageSize=1000"); // Request up to 1000 documents per page
            
            if let Some(token) = &page_token {
                url.push_str(&format!("&pageToken={}", token));
            }

            let response = self
                .client
                .get(&url)
                .bearer_auth(&self.access_token)
                .send()
                .await
                .map_err(|e| AuthError::new(&format!("Failed to get available tracks: {}", e)))?;

            match response.status().as_u16() {
                200 => {
                    let firestore_response: serde_json::Value = response.json().await.map_err(|e| {
                        AuthError::new(&format!("Failed to parse available tracks response: {}", e))
                    })?;

                    if let Some(documents) = firestore_response["documents"].as_array() {
                        for doc in documents {
                            if let Some(name) = doc["name"].as_str() {
                                // Extract track_id from document name
                                let track_id = name.split('/').last().unwrap_or("").to_string();
                                if !track_id.is_empty() {
                                    available_tracks.insert(track_id);
                                }
                            }
                        }
                    }

                    // Check if there's a next page
                    if let Some(next_page_token) = firestore_response["nextPageToken"].as_str() {
                        page_token = Some(next_page_token.to_string());
                    } else {
                        break; // No more pages
                    }
                }
                404 => break, // No documents found
                _ => {
                    let error_text = response.text().await.unwrap_or("Unknown error".to_string());
                    return Err(
                        AuthError::new(&format!("Failed to get available tracks: {}", error_text))
                            .into(),
                    );
                }
            }
        }

        Ok(available_tracks)
    }

    /// Add track to available tracks in Firebase
    pub async fn add_available_track(&self, track_id: &str) -> AuthResult<bool> {
        let url = self.get_document_path("available_tracks", track_id);

        let document_data = serde_json::json!({
            "fields": {
                "track_id": {
                    "stringValue": track_id
                },
                "added_at": {
                    "timestampValue": chrono::Utc::now().to_rfc3339()
                }
            }
        });

        let response = self
            .client
            .patch(&url)
            .bearer_auth(&self.access_token)
            .json(&document_data)
            .send()
            .await
            .map_err(|e| AuthError::new(&format!("Failed to add available track: {}", e)))?;

        if response.status().is_success() {
            Ok(true)
        } else {
            let error_text = response.text().await.unwrap_or("Unknown error".to_string());
            Err(AuthError::new(&format!("Failed to add available track: {}", error_text)).into())
        }
    }

    /// Remove track from available tracks in Firebase
    pub async fn remove_available_track(&self, track_id: &str) -> AuthResult<bool> {
        let url = self.get_document_path("available_tracks", track_id);

        let response = self
            .client
            .delete(&url)
            .bearer_auth(&self.access_token)
            .send()
            .await
            .map_err(|e| AuthError::new(&format!("Failed to remove available track: {}", e)))?;

        if response.status().is_success() {
            Ok(true)
        } else if response.status().as_u16() == 404 {
            Ok(false) // Track was not in available tracks
        } else {
            let error_text = response.text().await.unwrap_or("Unknown error".to_string());
            Err(AuthError::new(&format!("Failed to remove available track: {}", error_text)).into())
        }
    }
}
