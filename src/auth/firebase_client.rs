use serde::{Deserialize, Serialize};
use serde_json::Value;

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
    token_expires_at: chrono::DateTime<chrono::Utc>,
}

impl FirebaseClient {
    pub async fn new(auth_token: AuthToken) -> AuthResult<Self> {
        Ok(Self {
            client: reqwest::Client::new(),
            project_id: AppConfig::FIREBASE_PROJECT_ID.to_string(),
            user_id: auth_token.user_id,
            access_token: auth_token.access_token,
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

    /// Ensure token is valid, refresh if needed
    async fn ensure_valid_token(&mut self) -> AuthResult<()> {
        use crate::auth::google_auth::GoogleAuth;
        
        // Check if token expires within 5 minutes
        let expires_soon = self.token_expires_at - chrono::Duration::minutes(5);
        if chrono::Utc::now() > expires_soon {
            println!("🔄 Token expiring soon, refreshing authentication...");
            
            let new_token = GoogleAuth::new()
                .login()
                .await
                .map_err(|_| AuthError::new("Failed to refresh token"))?;
            
            self.access_token = new_token.access_token;
            self.token_expires_at = new_token.expires_at;
            
            println!("✅ Token refreshed successfully");
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
            },
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

    // Artist CRUD operations

    /// Save artist manager to Firestore
    pub async fn save_artists(&self, artist_manager: &ArtistManager) -> AuthResult<()> {
        let data = serde_json::to_value(artist_manager)
            .map_err(|e| AuthError::new(&format!("Serialization error: {}", e)))?;

        self.set_document("artists", "favorite_artists", &data)
            .await
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

    /// Add or update a specific artist
    pub async fn save_artist(&self, artist_name: &str, artist: &Artist) -> AuthResult<()> {
        let data = serde_json::to_value(artist)
            .map_err(|e| AuthError::new(&format!("Serialization error: {}", e)))?;

        let collection = format!("artists/favorite_artists/artists");
        self.set_document(&collection, artist_name, &data).await
    }

    /// Get a specific artist
    pub async fn get_artist(&self, artist_name: &str) -> AuthResult<Option<Artist>> {
        let collection = format!("artists/favorite_artists/artists");
        match self.get_document(&collection, artist_name).await? {
            Some(data) => {
                let artist: Artist = serde_json::from_value(data)
                    .map_err(|e| AuthError::new(&format!("Deserialization error: {}", e)))?;
                Ok(Some(artist))
            }
            None => Ok(None),
        }
    }

    /// Delete a specific artist
    pub async fn delete_artist(&self, artist_name: &str) -> AuthResult<()> {
        let collection = format!("artists/favorite_artists/artists");
        let url = format!(
            "{}/{}/{}",
            self.firestore_url(),
            self.user_collection_path(&collection),
            artist_name
        );

        let response = self
            .client
            .delete(&url)
            .header("Authorization", format!("Bearer {}", self.access_token))
            .send()
            .await
            .map_err(|e| AuthError::new(&format!("Failed to delete artist: {}", e)))?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(AuthError::new(&format!("Firestore delete error: {}", error_text)).into());
        }

        Ok(())
    }

    // Genre Tracker CRUD operations

    /// Save genre tracker to Firestore
    pub async fn save_genre_tracker(&self, genre_tracker: &GenreTracker) -> AuthResult<()> {
        let data = serde_json::to_value(genre_tracker)
            .map_err(|e| AuthError::new(&format!("Serialization error: {}", e)))?;

        self.set_document("genre_tracker", "main", &data).await
    }

    /// Load genre tracker from Firestore
    pub async fn load_genre_tracker(&self) -> AuthResult<Option<GenreTracker>> {
        match self.get_document("genre_tracker", "main").await? {
            Some(data) => {
                let genre_tracker: GenreTracker = serde_json::from_value(data)
                    .map_err(|e| AuthError::new(&format!("Deserialization error: {}", e)))?;
                Ok(Some(genre_tracker))
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
        self.save_genre_tracker(&genre_tracker).await?;

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
    pub async fn save_track(&self, track_id: &str, track: &crate::soundeo::track::SoundeoTrack) -> AuthResult<()> {
        let data = serde_json::to_value(track)
            .map_err(|e| AuthError::new(&format!("Serialization error: {}", e)))?;

        // Use simple flat collection structure for O(1) access
        self.set_document("soundeo_tracks", track_id, &data).await
    }

    /// Get a single track from Firebase by ID (O(1) access)
    pub async fn get_track(&self, track_id: &str) -> AuthResult<Option<crate::soundeo::track::SoundeoTrack>> {
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
    pub async fn get_tracks(&self, track_ids: &[String]) -> AuthResult<std::collections::HashMap<String, crate::soundeo::track::SoundeoTrack>> {
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
    pub async fn batch_write_tracks(&mut self, tracks: &[(String, crate::soundeo::track::SoundeoTrack)]) -> AuthResult<()> {
        self.ensure_valid_token().await?;
        
        if tracks.is_empty() {
            return Ok(());
        }

        // Firebase batch write limit is 500 operations
        if tracks.len() > 500 {
            return Err(AuthError::new("Batch size exceeds Firebase limit of 500 operations").into());
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
            fields.insert("id".to_string(), self.convert_to_firestore_value(&serde_json::Value::String(track.id.clone())));
            fields.insert("title".to_string(), self.convert_to_firestore_value(&serde_json::Value::String(track.title.clone())));
            fields.insert("track_url".to_string(), self.convert_to_firestore_value(&serde_json::Value::String(track.track_url.clone())));
            fields.insert("date".to_string(), self.convert_to_firestore_value(&serde_json::Value::String(track.date.clone())));
            fields.insert("genre".to_string(), self.convert_to_firestore_value(&serde_json::Value::String(track.genre.clone())));
            fields.insert("downloadable".to_string(), self.convert_to_firestore_value(&serde_json::Value::Bool(track.downloadable)));
            fields.insert("already_downloaded".to_string(), self.convert_to_firestore_value(&serde_json::Value::Bool(track.already_downloaded)));

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

        let response = self.client
            .post(&batch_url)
            .bearer_auth(&self.access_token)
            .json(&batch_request)
            .send()
            .await
            .map_err(|e| AuthError::new(&format!("HTTP request failed: {}", e)))?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(AuthError::new(&format!("Firestore batch write error: {}", error_text)).into());
        }

        Ok(())
    }

    /// Migrate queue tracks to priority-based subcollections
    pub async fn migrate_queue_to_subcollections(&mut self, queued_tracks: &[crate::log::QueuedTrack]) -> AuthResult<()> {
        self.ensure_valid_token().await?;
        let total = queued_tracks.len();
        let start_time = std::time::Instant::now();
        let mut completed = 0;
        let mut failed = 0;
        
        for (index, track) in queued_tracks.iter().enumerate() {
            // Check token every 100 tracks or if this is the first track
            if index == 0 || index % 100 == 0 {
                self.ensure_valid_token().await?;
            }
            
            let priority_name = match track.priority {
                crate::log::Priority::High => "high",
                crate::log::Priority::Normal => "normal", 
                crate::log::Priority::Low => "low",
            };
            
            let url = format!(
                "{}/users/{}/queued_tracks/{}",
                self.firestore_url(), self.user_id, track.track_id
            );
            
            // Update stats line
            let elapsed = start_time.elapsed();
            let processed = completed + failed;
            let rate = if elapsed.as_secs() > 0 {
                (completed as f64 / elapsed.as_secs_f64()) * 60.0
            } else {
                0.0
            };
            let eta = if rate > 0.0 {
                let remaining = total - processed;
                let minutes_remaining = remaining as f64 / rate;
                if minutes_remaining < 60.0 {
                    format!("{:.0}m", minutes_remaining)
                } else {
                    format!("{:.1}h", minutes_remaining / 60.0)
                }
            } else {
                "∞".to_string()
            };
            
            print!("\r📋 Queue: {}/{} | ✅ {} ❌ {} | ⏱️ {:?} | 🚀 {:.1}/min | ⏳ {}", 
                   processed, total, completed, failed, elapsed, rate, eta);
            std::io::Write::flush(&mut std::io::stdout()).unwrap_or_default();
            
            let result = self.client
                .patch(&url)
                .bearer_auth(&self.access_token)
                .query(&[("updateMask.fieldPaths", "track_id"), ("updateMask.fieldPaths", "order_key"), ("updateMask.fieldPaths", "priority")])
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
                    completed += 1;
                    // Mark track as migrated in local JSON
                    let _ = crate::log::DjWizardLog::mark_queued_track_as_migrated(&track.track_id);
                }
                Ok(_) | Err(_) => {
                    failed += 1;
                }
            }
        }
        
        // Final stats
        let final_elapsed = start_time.elapsed();
        let final_rate = if final_elapsed.as_secs() > 0 {
            (completed as f64 / final_elapsed.as_secs_f64()) * 60.0
        } else {
            0.0
        };
        
        println!("\r📋 Queue: {}/{} | ✅ {} ❌ {} | ⏱️ {:?} | 🚀 {:.1}/min | ✅ Complete!", 
                 total, total, completed, failed, final_elapsed, final_rate);
        
        if failed > 0 {
            return Err(AuthError::new(&format!("Migration completed with {} failures", failed)).into());
        }
        
        Ok(())
    }

}
