use std::{env, fmt};

use clap::{Parser, Subcommand};
use colored::Colorize;
use error_stack::fmt::{Charset, ColorMode};
use error_stack::{FutureExt, IntoReport, Report, ResultExt};
use native_dialog::FileDialog;

use crate::artist::commands::ArtistCommands;
use crate::backup::commands::BackupCommands;
use crate::cleaner::clean_repeated_files;
use crate::dialoguer::Dialoguer;
use crate::genre_tracker::commands::GenreTrackerCommands;
use crate::log::DjWizardLog;
use crate::queue::commands::QueueCommands;
use crate::soundeo::track::SoundeoTrack;
use crate::spotify::commands::{SpotifyCli, SpotifyCommands};
use crate::url_list::commands::UrlListCommands;
use crate::user::{SoundeoUser, User};

mod artist;
mod auth;
mod backup;
mod cleaner;
mod config;
mod dialoguer;
mod errors;
mod genre_tracker;
mod ipfs;
mod log;
mod queue;
mod soundeo;
mod spotify;
mod url_list;
mod user;

#[derive(Debug)]
pub struct DjWizardError;
impl fmt::Display for DjWizardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Dj Wizard error")
    }
}
impl std::error::Error for DjWizardError {}

pub type DjWizardResult<T> = error_stack::Result<T, DjWizardError>;

#[derive(Parser, Debug, Clone)]
#[command(author, version, about = "Dj Wizard bot")]
struct Cli {
    #[command(subcommand)]
    command: DjWizardCommands,
}

/// A simple program to download all files from a search in soundeo
#[derive(Subcommand, Debug, PartialEq, Clone)]
enum DjWizardCommands {
    /// Authenticate with Google for cloud sync
    Auth,
    /// Stores the Soundeo credentials
    Login,
    /// Stores the IPFS credentials
    IPFS,
    /// Reads the current config file
    Config,
    /// Add tracks to a queue or resumes the download from it
    Queue {
        /// flag to repet already downloaded
        #[clap(long, short, action)]
        resume_queue: bool,
    },
    /// Add all the tracks from a url to the Soundeo collection and queue them
    Url,
    /// Clean all the repeated files starting on a path.
    /// Thought to be used to clean repeated files
    /// on DJ programs e.g. Rekordbox
    Clean,
    /// Get Soundeo track info by id
    Info,
    /// Automatically download tracks from a Spotify playlist
    Spotify(SpotifyCli),
    /// Backup the log file to the cloud
    Backup,
    /// Track available tracks by genre
    Genre,
    /// Manage favorite artists
    Artist,
    /// Migrate soundeo_log.json to Firebase
    Migrate {
        /// Path to soundeo_log.json file
        #[clap(long)]
        soundeo_log: Option<String>,
        /// Only migrate light fields (exclude soundeo and queued_tracks)
        #[clap(long)]
        light_only: bool,
        /// Migrate queued tracks
        #[clap(long)]
        queued_tracks: bool,
        /// Migrate soundeo field
        #[clap(long)]
        soundeo: bool,
        /// Migrate all remaining fields (tracks_info, etc.)
        #[clap(long)]
        remaining: bool,
    },
}

impl DjWizardCommands {
    pub async fn execute(&self) -> DjWizardResult<()> {
        return match self {
            DjWizardCommands::Auth => {
                use crate::auth::google_auth::GoogleAuth;

                let google_auth = GoogleAuth::new();
                let result = google_auth.login().await;
                match result {
                    Ok(_token) => {
                        println!(
                            "Authentication successful! Your data will now sync to the cloud."
                        );
                        Ok(())
                    }
                    Err(_) => {
                        println!("Authentication failed. Please try again.");
                        Err(DjWizardError).into_report()
                    }
                }
            }
            DjWizardCommands::Login => {
                let mut soundeo_user_config = User::new();
                let prompt_text = format!("Soundeo user: ");
                soundeo_user_config.soundeo_user =
                    Dialoguer::input(prompt_text).change_context(DjWizardError)?;
                let prompt_text = format!("Password: ");
                soundeo_user_config.soundeo_pass =
                    Dialoguer::password(prompt_text).change_context(DjWizardError)?;
                let home_path = env::var("HOME")
                    .into_report()
                    .change_context(DjWizardError)?;
                let selected_path = FileDialog::new()
                    .set_location(&home_path)
                    .show_open_single_dir()
                    .into_report()
                    .change_context(DjWizardError)?
                    .ok_or(DjWizardError)
                    .into_report()?;
                soundeo_user_config.download_path = selected_path
                    .to_str()
                    .ok_or(DjWizardError)
                    .into_report()?
                    .to_string();
                println!(
                    "Soundeo credentials successfully stored:\n {:#?}",
                    soundeo_user_config
                );
                soundeo_user_config
                    .create_new_config_file()
                    .change_context(DjWizardError)?;
                Ok(())
            }

            DjWizardCommands::IPFS => {
                let options = vec!["Upload log to IPFS", "Update IPFS credentials"];
                let prompt_text = "What you want to do?".to_string();
                let selection =
                    Dialoguer::select(prompt_text, options, None).change_context(DjWizardError)?;
                if selection == 0 {
                    DjWizardLog::upload_to_ipfs().change_context(DjWizardError)?;
                } else {
                    let mut soundeo_user_config = User::new();
                    soundeo_user_config
                        .read_config_file()
                        .change_context(DjWizardError)?;
                    let prompt_text = format!("IPFS api key: ");
                    soundeo_user_config.ipfs.api_key =
                        Dialoguer::input(prompt_text).change_context(DjWizardError)?;
                    let prompt_text = format!("IPFS api key secret: ");
                    soundeo_user_config.ipfs.api_key_secret =
                        Dialoguer::password(prompt_text).change_context(DjWizardError)?;
                    println!(
                        "IPFS credentials successfully stored:\n {:#?}",
                        soundeo_user_config.ipfs
                    );
                    soundeo_user_config
                        .save_config_file()
                        .change_context(DjWizardError)?;
                }
                Ok(())
            }
            DjWizardCommands::Config => {
                let mut soundeo_bot_config = User::new();
                soundeo_bot_config
                    .read_config_file()
                    .change_context(DjWizardError)?;
                println!("Current config:\n{:#?}", soundeo_bot_config);
                Ok(())
            }
            DjWizardCommands::Queue { resume_queue } => {
                QueueCommands::execute(*resume_queue)
                    .change_context(DjWizardError)
                    .await
            }
            DjWizardCommands::Url => {
                UrlListCommands::execute()
                    .change_context(DjWizardError)
                    .await
            }
            DjWizardCommands::Clean => {
                let soundeo_user = SoundeoUser::new().change_context(DjWizardError)?;
                println!("Select the folder to start cleaning repeated files");
                let selected_path = FileDialog::new()
                    .set_location(&soundeo_user.download_path)
                    .show_open_single_dir()
                    .into_report()
                    .change_context(DjWizardError)?
                    .ok_or(DjWizardError)
                    .into_report()?;
                println!(
                    "Cleaning {}",
                    selected_path.clone().to_str().unwrap().cyan()
                );
                clean_repeated_files(selected_path).change_context(DjWizardError)
            }
            DjWizardCommands::Info => {
                let prompt_text = "Soundeo track id: ".to_string();
                let track_id = Dialoguer::input(prompt_text).change_context(DjWizardError)?;
                let mut soundeo_user = SoundeoUser::new().change_context(DjWizardError)?;
                soundeo_user
                    .login_and_update_user_info()
                    .await
                    .change_context(DjWizardError)?;
                let mut soundeo_track_full_info = SoundeoTrack::new(track_id.clone());
                soundeo_track_full_info
                    .get_info(&soundeo_user, true)
                    .await
                    .change_context(DjWizardError)?;
                println!("{:#?}", soundeo_track_full_info);
                Ok(())
            }
            DjWizardCommands::Spotify(cli) => {
                SpotifyCommands::execute(Some(cli.clone()))
                    .change_context(DjWizardError)
                    .await
            }
            DjWizardCommands::Backup => {
                BackupCommands::execute()
                    .change_context(DjWizardError)
                    .await
            }
            DjWizardCommands::Genre => {
                GenreTrackerCommands::execute()
                    .change_context(DjWizardError)
                    .await
            }
            DjWizardCommands::Artist => ArtistCommands::execute().change_context(DjWizardError),
            DjWizardCommands::Migrate {
                soundeo_log,
                light_only,
                queued_tracks,
                soundeo,
                remaining,
            } => {
                use crate::auth::firebase_client::FirebaseClient;
                use crate::auth::google_auth::GoogleAuth;

                // Try to load existing token
                let auth_token = match GoogleAuth::load_token() {
                    Ok(token) => token,
                    Err(_) => {
                        println!(
                            "❌ No valid authentication found. Please run 'dj-wizard auth' first."
                        );
                        return Err(DjWizardError).into_report();
                    }
                };

                // Create Firebase client
                let firebase_client = FirebaseClient::new(auth_token)
                    .await
                    .change_context(DjWizardError)?;

                // Set default path if not provided
                let default_log_path = "/Users/matiasbn/soundeo-bot-files/soundeo_log.json";
                let log_path = soundeo_log.as_deref().unwrap_or(default_log_path);

                println!("🔄 Migrating complete soundeo_log.json to Firebase...");
                println!("📂 Reading from: {}", log_path);

                // Check file size first
                let metadata = std::fs::metadata(log_path).map_err(|e| {
                    println!("❌ Failed to read file metadata: {}", e);
                    DjWizardError
                })?;

                let file_size_mb = metadata.len() as f64 / 1024.0 / 1024.0;
                println!("📊 File size: {:.2} MB", file_size_mb);

                if file_size_mb > 1.0 {
                    println!("⚠️  Warning: File is larger than 1MB. Firebase might reject it.");
                    println!("💡 Consider using a smaller test file first.");
                }

                // Read the entire JSON file
                println!("📖 Reading file contents...");
                let log_data = std::fs::read_to_string(log_path).map_err(|e| {
                    println!("❌ Failed to read log file: {}", e);
                    DjWizardError
                })?;

                println!("✅ File read successfully ({} bytes)", log_data.len());

                // Parse as JSON to validate it's correct
                println!("🔍 Parsing JSON...");
                let json_value: serde_json::Value =
                    serde_json::from_str(&log_data).map_err(|e| {
                        println!("❌ Invalid JSON file: {}", e);
                        DjWizardError
                    })?;

                println!("✅ JSON parsed successfully");

                if *queued_tracks {
                    // Special mode: add queued tracks to existing document
                    println!("🎯 Queued tracks mode: Adding to existing document...");

                    if let serde_json::Value::Object(map) = &json_value {
                        if let Some(serde_json::Value::Array(tracks)) = map.get("queued_tracks") {
                            println!("📊 Found {} queued tracks", tracks.len());

                            // Get existing document first
                            println!("📥 Getting existing document...");
                            let mut existing_doc = firebase_client
                                .get_document("dj_wizard_data", "soundeo_log")
                                .await
                                .change_context(DjWizardError)?
                                .unwrap_or_else(|| serde_json::json!({}));

                            // Add queued_tracks to existing document
                            if let serde_json::Value::Object(ref mut existing_map) = existing_doc {
                                existing_map.insert(
                                    "queued_tracks".to_string(),
                                    serde_json::Value::Array(tracks.clone()),
                                );
                                println!("✅ Added queued_tracks to existing document");
                            } else {
                                // If not an object, create new structure
                                existing_doc = serde_json::json!({
                                    "queued_tracks": tracks
                                });
                                println!("✅ Created new document with queued_tracks");
                            }

                            // Upload updated document
                            println!("☁️  Uploading updated document...");
                            firebase_client
                                .set_document("dj_wizard_data", "soundeo_log", &existing_doc)
                                .await
                                .change_context(DjWizardError)?;

                            println!(
                                "🎉 Successfully added {} queued tracks to existing document!",
                                tracks.len()
                            );
                            return Ok(());
                        } else {
                            println!("❌ No 'queued_tracks' field found in JSON");
                            return Err(DjWizardError).into_report();
                        }
                    } else {
                        println!("❌ JSON is not an object");
                        return Err(DjWizardError).into_report();
                    }
                }

                if *soundeo {
                    // Use existing DjWizardLog logic to read soundeo properly
                    println!("🎵 Soundeo mode: Using existing log reader...");

                    // Read soundeo using the existing working logic
                    use crate::log::DjWizardLog;
                    let current_soundeo =
                        DjWizardLog::get_soundeo().change_context(DjWizardError)?;

                    println!(
                        "📊 Found soundeo with {} tracks_info",
                        current_soundeo.tracks_info.len()
                    );

                    // Get existing document first
                    println!("📥 Getting existing document...");
                    let mut existing_doc = firebase_client
                        .get_document("dj_wizard_data", "soundeo_log")
                        .await
                        .change_context(DjWizardError)?
                        .unwrap_or_else(|| serde_json::json!({}));

                    if current_soundeo.tracks_info.is_empty() {
                        // No tracks_info, just add soundeo structure
                        let soundeo_value = serde_json::to_value(&current_soundeo)
                            .into_report()
                            .change_context(DjWizardError)?;

                        if let serde_json::Value::Object(ref mut existing_map) = existing_doc {
                            existing_map.insert("soundeo".to_string(), soundeo_value);
                        } else {
                            existing_doc = serde_json::json!({
                                "soundeo": soundeo_value
                            });
                        }

                        firebase_client
                            .set_document("dj_wizard_data", "soundeo_log", &existing_doc)
                            .await
                            .change_context(DjWizardError)?;

                        println!("🎉 Successfully added empty soundeo field!");
                    } else {
                        // Has tracks_info - migrate in batches
                        let tracks: Vec<_> = current_soundeo.tracks_info.into_iter().collect();

                        // Initialize soundeo structure in document first (empty tracks_info)
                        let empty_soundeo = crate::soundeo::Soundeo::new();
                        let soundeo_value = serde_json::to_value(&empty_soundeo)
                            .into_report()
                            .change_context(DjWizardError)?;

                        if let serde_json::Value::Object(ref mut existing_map) = existing_doc {
                            existing_map.insert("soundeo".to_string(), soundeo_value);
                        } else {
                            existing_doc = serde_json::json!({
                                "soundeo": soundeo_value
                            });
                        }

                        // Migrate tracks_info with sliding window parallel processing
                        let batch_size = 200;
                        let total_batches = (tracks.len() + batch_size - 1) / batch_size;
                        let concurrent_limit = 3;

                        println!("🚀 Starting sliding window parallel migration with {} concurrent slots", concurrent_limit);

                        use futures_util::stream::{FuturesUnordered, StreamExt};
                        use std::pin::Pin;
                        use std::future::Future;
                        use std::sync::Arc;
                        
                        type BatchFuture = Pin<Box<dyn Future<Output = Result<(usize, serde_json::Value), error_stack::Report<crate::auth::AuthError>>> + Send>>;
                        
                        let mut active_futures: FuturesUnordered<BatchFuture> = FuturesUnordered::new();
                        let mut next_batch_idx = 0;
                        let mut completed_batches = 0;
                        
                        // Wrap firebase_client in Arc for sharing across futures
                        let firebase_client = Arc::new(firebase_client);

                        // Helper function to create a batch upload future
                        let create_batch_future = |batch_num: usize, batch_doc: serde_json::Value, chunk_len: usize, client: Arc<FirebaseClient>| -> BatchFuture {
                            Box::pin(async move {
                                println!("📤 Starting batch {} ({} tracks)...", batch_num, chunk_len);
                                
                                let result = client
                                    .set_document("dj_wizard_data", "soundeo_log", &batch_doc)
                                    .await;
                                
                                match result {
                                    Ok(_) => {
                                        println!("✅ Batch {} completed successfully", batch_num);
                                        Ok((batch_num, batch_doc))
                                    }
                                    Err(e) => {
                                        println!("❌ Batch {} failed: {:?}", batch_num, e);
                                        Err(e)
                                    }
                                }
                            })
                        };
                        
                        // Helper function to prepare batch data
                        let prepare_batch_data = |batch_idx: usize, existing_doc: &serde_json::Value| -> DjWizardResult<(usize, serde_json::Value, usize)> {
                            let chunk_start = batch_idx * batch_size;
                            let chunk_end = ((batch_idx + 1) * batch_size).min(tracks.len());
                            let chunk = &tracks[chunk_start..chunk_end];
                            
                            // Clone and build the batch document
                            let mut batch_doc = existing_doc.clone();
                            let batch_num = batch_idx + 1;
                            
                            if let serde_json::Value::Object(ref mut doc_map) = batch_doc {
                                if let Some(serde_json::Value::Object(ref mut soundeo_map)) =
                                    doc_map.get_mut("soundeo")
                                {
                                    if let Some(serde_json::Value::Object(
                                        ref mut tracks_info_map,
                                    )) = soundeo_map.get_mut("tracks_info")
                                    {
                                        for (track_id, track) in chunk {
                                            let track_value = serde_json::to_value(track)
                                                .into_report()
                                                .change_context(DjWizardError)?;
                                            tracks_info_map.insert(track_id.clone(), track_value);
                                        }
                                    }
                                }
                            }
                            
                            Ok((batch_num, batch_doc, chunk.len()))
                        };

                        // Start initial batch of concurrent uploads
                        while next_batch_idx < total_batches && active_futures.len() < concurrent_limit {
                            match prepare_batch_data(next_batch_idx, &existing_doc) {
                                Ok((batch_num, batch_doc, chunk_len)) => {
                                    let future = create_batch_future(batch_num, batch_doc, chunk_len, firebase_client.clone());
                                    active_futures.push(future);
                                    next_batch_idx += 1;
                                }
                                Err(e) => return Err(e),
                            }
                        }

                        // Process completions and start new batches immediately
                        while !active_futures.is_empty() {
                            if let Some(result) = active_futures.next().await {
                                match result {
                                    Ok((batch_num, batch_doc)) => {
                                        completed_batches += 1;
                                        
                                        // Update our local state with the successful batch
                                        existing_doc = batch_doc;
                                        
                                        println!(
                                            "🎯 Progress: {}/{} batches completed",
                                            completed_batches,
                                            total_batches
                                        );
                                        
                                        // Immediately start the next batch if available
                                        if next_batch_idx < total_batches {
                                            match prepare_batch_data(next_batch_idx, &existing_doc) {
                                                Ok((new_batch_num, new_batch_doc, chunk_len)) => {
                                                    let future = create_batch_future(new_batch_num, new_batch_doc, chunk_len, firebase_client.clone());
                                                    active_futures.push(future);
                                                    next_batch_idx += 1;
                                                }
                                                Err(e) => return Err(e),
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        return Err(e.change_context(DjWizardError));
                                    }
                                }
                            }
                        }

                        println!(
                            "🎉 Successfully migrated all {} tracks to soundeo.tracks_info!",
                            tracks.len()
                        );
                    }

                    return Ok(());
                }

                if *remaining {
                    // Special mode: add any remaining fields to existing document
                    println!("📦 Remaining fields mode: Adding all missing fields...");

                    if let serde_json::Value::Object(map) = &json_value {
                        // Get existing document first
                        println!("📥 Getting existing document...");
                        let mut existing_doc = firebase_client
                            .get_document("dj_wizard_data", "soundeo_log")
                            .await
                            .change_context(DjWizardError)?
                            .unwrap_or_else(|| serde_json::json!({}));

                        let mut added_fields = Vec::new();

                        // Add any missing fields to existing document
                        if let serde_json::Value::Object(ref mut existing_map) = existing_doc {
                            for (key, value) in map.iter() {
                                if !existing_map.contains_key(key) {
                                    existing_map.insert(key.clone(), value.clone());
                                    added_fields.push(key.clone());
                                }
                            }
                        } else {
                            // If not an object, create new structure with all fields
                            existing_doc = serde_json::Value::Object(map.clone());
                            added_fields = map.keys().cloned().collect();
                        }

                        if added_fields.is_empty() {
                            println!(
                                "ℹ️  No new fields to add - document already contains all data"
                            );
                            return Ok(());
                        }

                        println!("✅ Added fields: {:?}", added_fields);

                        // Upload updated document
                        println!("☁️  Uploading updated document...");
                        firebase_client
                            .set_document("dj_wizard_data", "soundeo_log", &existing_doc)
                            .await
                            .change_context(DjWizardError)?;

                        println!(
                            "🎉 Successfully added {} remaining fields to existing document!",
                            added_fields.len()
                        );
                        return Ok(());
                    } else {
                        println!("❌ JSON is not an object");
                        return Err(DjWizardError).into_report();
                    }
                }

                let final_data = if *light_only {
                    println!("🪶 Light mode: Filtering out heavy fields...");

                    // Extract only light fields, exclude heavy ones
                    if let serde_json::Value::Object(mut map) = json_value {
                        // Remove heavy fields
                        let removed_soundeo = map.remove("soundeo");
                        let removed_queued = map.remove("queued_tracks");

                        if removed_soundeo.is_some() {
                            println!("🗑️  Excluded 'soundeo' field");
                        }
                        if removed_queued.is_some() {
                            println!("🗑️  Excluded 'queued_tracks' field");
                        }

                        println!("✅ Remaining fields: {:?}", map.keys().collect::<Vec<_>>());
                        serde_json::Value::Object(map)
                    } else {
                        println!("⚠️  JSON is not an object, uploading as-is");
                        json_value
                    }
                } else {
                    println!("📦 Full mode: Uploading complete file...");
                    json_value
                };

                // Show final size
                let final_size = serde_json::to_string(&final_data).unwrap().len();
                let final_size_mb = final_size as f64 / 1024.0 / 1024.0;
                println!(
                    "📊 Final upload size: {:.2} MB ({} bytes)",
                    final_size_mb, final_size
                );

                // Upload the data
                println!("☁️  Uploading to Firebase...");
                firebase_client
                    .set_document("dj_wizard_data", "soundeo_log", &final_data)
                    .await
                    .change_context(DjWizardError)?;

                println!("✅ Successfully migrated entire soundeo_log.json to Firebase!");
                println!("🎉 Your data is now available in the cloud!");
                Ok(())
            }
        };
    }

    pub fn cli_command(&self) -> String {
        match self {
            DjWizardCommands::Auth => {
                format!("dj-wizard auth")
            }
            DjWizardCommands::Login => {
                format!("dj-wizard login")
            }
            DjWizardCommands::Config => {
                format!("dj-wizard config")
            }
            DjWizardCommands::Queue { .. } => {
                format!("dj-wizard queue")
            }
            DjWizardCommands::Url => {
                format!("dj-wizard url")
            }
            DjWizardCommands::Clean => {
                format!("dj-wizard clean")
            }
            DjWizardCommands::Info => {
                format!("dj-wizard info")
            }
            DjWizardCommands::Spotify(..) => {
                format!("dj-wizard spotify")
            }
            DjWizardCommands::IPFS => {
                format!("dj-wizard ipfs")
            }
            DjWizardCommands::Backup => {
                format!("dj-wizard backup")
            }
            DjWizardCommands::Genre => {
                format!("dj-wizard genre")
            }
            DjWizardCommands::Artist => {
                format!("dj-wizard artist")
            }
            DjWizardCommands::Migrate {
                soundeo_log,
                light_only,
                queued_tracks,
                soundeo,
                remaining,
            } => {
                let mut cmd = "dj-wizard migrate".to_string();
                if let Some(log_path) = soundeo_log {
                    cmd.push_str(&format!(" --soundeo-log {}", log_path));
                }
                if *light_only {
                    cmd.push_str(" --light-only");
                }
                if *queued_tracks {
                    cmd.push_str(" --queued-tracks");
                }
                if *soundeo {
                    cmd.push_str(" --soundeo");
                }
                if *remaining {
                    cmd.push_str(" --remaining");
                }
                cmd
            }
        }
    }
}

pub struct Suggestion(String);

impl Suggestion {
    pub fn set_report() {
        Report::set_charset(Charset::Utf8);
        Report::set_color_mode(ColorMode::Color);
        Report::install_debug_hook::<Self>(|Self(value), context| {
            context.push_body(format!("{}: {value}", "suggestion".yellow()))
        });
    }
}

async fn run() -> DjWizardResult<()> {
    let cli = Cli::parse();

    Suggestion::set_report();

    cli.command.execute().await?;

    // Ok(())
    Ok(())
}

#[tokio::main]
async fn main() -> DjWizardResult<()> {
    run().await
}
