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
}

impl FirebaseClient {
    pub async fn new(auth_token: AuthToken) -> AuthResult<Self> {
        Ok(Self {
            client: reqwest::Client::new(),
            project_id: AppConfig::FIREBASE_PROJECT_ID.to_string(),
            user_id: auth_token.user_id,
            access_token: auth_token.access_token,
        })
    }

    /// Get the base URL for Firestore REST API
    fn firestore_url(&self) -> String {
        format!(
            "https://firestore.googleapis.com/v1/projects/{}/databases/(default)/documents",
            self.project_id
        )
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
            Value::Number(n) => serde_json::json!({"integerValue": n.to_string()}),
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
}
