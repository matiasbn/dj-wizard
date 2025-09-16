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
        /// Migrate tracks as individual documents for O(1) access (super fast)
        #[clap(long)]
        individual_tracks: bool,
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
                individual_tracks,
            } => {
                use crate::auth::firebase_client::FirebaseClient;
                use crate::auth::google_auth::GoogleAuth;

                // Try to load existing token, refresh if needed
                let auth_token = match GoogleAuth::load_token() {
                    Ok(token) => {
                        // Check if token is about to expire (within 5 minutes)
                        let expires_soon = token.expires_at - chrono::Duration::minutes(5);
                        if chrono::Utc::now() > expires_soon {
                            println!("🔄 Token expires soon, refreshing authentication...");
                            GoogleAuth::new()
                                .login()
                                .await
                                .change_context(DjWizardError)?
                        } else {
                            token
                        }
                    }
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
                    // Ultra fast individual track migration with O(1) access using same parallelism
                    println!("⚡ Soundeo mode: Migrating tracks as individual documents for O(1) access...");

                    // Read soundeo using the existing working logic
                    use crate::log::DjWizardLog;
                    let current_soundeo =
                        DjWizardLog::get_soundeo().change_context(DjWizardError)?;

                    println!(
                        "📊 Found soundeo with {} tracks_info for ultra-fast individual migration",
                        current_soundeo.tracks_info.len()
                    );

                    if current_soundeo.tracks_info.is_empty() {
                        println!("ℹ️  No tracks to migrate in soundeo mode");
                        return Ok(());
                    }

                    // Filter only non-migrated tracks
                    let total_all_tracks = current_soundeo.tracks_info.len();
                    let non_migrated_tracks: Vec<_> = current_soundeo
                        .tracks_info
                        .into_iter()
                        .filter(|(_, track)| !track.migrated)
                        .collect();

                    let total_tracks = non_migrated_tracks.len();

                    println!(
                        "📊 Found {} total tracks, {} already migrated, {} pending migration",
                        total_all_tracks,
                        total_all_tracks - total_tracks,
                        total_tracks
                    );

                    if total_tracks == 0 {
                        println!("✅ All tracks already migrated!");
                        return Ok(());
                    }

                    let tracks = non_migrated_tracks;
                    let concurrent_limit = 8; // 5 threads with persistent status lines
                    let max_retries = 3;

                    // Test Firebase connectivity first
                    println!("🔍 Testing Firebase connectivity...");
                    match firebase_client
                        .get_document("test", "connectivity_test")
                        .await
                    {
                        Ok(_) => println!("✅ Firebase connection OK"),
                        Err(e) => {
                            println!("❌ Firebase connection failed: {}", e);
                            println!("⚠️  Check your internet connection and Firebase permissions");
                            return Err(DjWizardError).into_report();
                        }
                    }

                    println!(
                        "🚀 Starting individual track migration with {} concurrent threads",
                        concurrent_limit
                    );

                    use futures_util::stream::{FuturesUnordered, StreamExt};
                    use std::future::Future;
                    use std::io::{self, Write};
                    use std::pin::Pin;
                    use std::sync::atomic::{AtomicUsize, Ordering};
                    use std::sync::Arc;

                    type TrackFuture = Pin<
                        Box<
                            dyn Future<
                                    Output = Result<
                                        String,
                                        error_stack::Report<crate::auth::AuthError>,
                                    >,
                                > + Send,
                        >,
                    >;

                    // Shared progress counters
                    let completed_count = Arc::new(AtomicUsize::new(0));
                    let failed_count = Arc::new(AtomicUsize::new(0));
                    let firebase_client = Arc::new(firebase_client);

                    // Migration timing
                    let start_time = std::time::Instant::now();
                    let start_time_shared = Arc::new(start_time);

                    // Helper function to update real-time stats
                    let update_stats =
                        |completed: usize,
                         failed: usize,
                         total: usize,
                         start_time: std::time::Instant| {
                            let elapsed = start_time.elapsed();
                            let processed = completed + failed;
                            let avg_time = if completed > 0 {
                                elapsed / completed as u32
                            } else {
                                std::time::Duration::from_secs(0)
                            };
                            let rate = if elapsed.as_secs() > 0 {
                                (completed as f64 / elapsed.as_secs_f64()) * 60.0
                            } else {
                                0.0
                            };

                            format!(
                                "📊 {}/{} | ✅ {} ❌ {} | ⏱️ {:?} | 📈 {:.2?}/track | 🚀 {:.1}/min",
                                processed, total, completed, failed, elapsed, avg_time, rate
                            )
                        };

                    // Helper function to create individual track upload future with retry
                    let create_track_future = |track_id: String,
                                               track: crate::soundeo::track::SoundeoTrack,
                                               client: Arc<FirebaseClient>,
                                               completed: Arc<AtomicUsize>,
                                               failed: Arc<AtomicUsize>,
                                               total: usize,
                                               thread_id: usize,
                                               start_time: Arc<std::time::Instant>|
                     -> TrackFuture {
                        Box::pin(async move {
                            let mut retry_count = 0;

                            // First check if track already exists in Firebase
                            match client.track_exists(&track_id).await {
                                Ok(true) => {
                                    // Track already exists, mark as migrated and skip
                                    let current_completed =
                                        completed.fetch_add(1, Ordering::Relaxed) + 1;
                                    let current_failed = failed.load(Ordering::Relaxed);

                                    // Move to statistics line (first line), update it
                                    print!("\x1B[{}A", concurrent_limit + 1 - thread_id); // Move up to stats line
                                    print!("\x1B[2K\r{}", update_stats(current_completed, current_failed, total, *start_time));

                                    // Move to this thread's line, update it  
                                    print!("\x1B[{}A", concurrent_limit - thread_id); // Move up to thread line
                                    print!("\x1B[2K\rThread {}: ⏭️  {} already exists ({}/{})", 
                                            thread_id, track_id, current_completed + current_failed, total);
                                    print!("\x1B[{}B", concurrent_limit - thread_id); // Move back down
                                    io::stdout().flush().unwrap_or_default();

                                    // Mark as migrated in local JSON
                                    if let Err(e) = DjWizardLog::mark_track_as_migrated(&track_id) {
                                        eprintln!(
                                            "⚠️  Failed to mark track {} as migrated: {}",
                                            track_id, e
                                        );
                                    }

                                    return Ok(track_id);
                                }
                                Ok(false) => {
                                    // Track doesn't exist, proceed with upload
                                }
                                Err(e) => {
                                    // Error checking, log but proceed with upload attempt
                                    eprintln!(
                                        "⚠️  Failed to check if track {} exists: {}",
                                        track_id, e
                                    );
                                }
                            }

                            loop {
                                match client.save_track(&track_id, &track).await {
                                    Ok(_) => {
                                        let current_completed =
                                            completed.fetch_add(1, Ordering::Relaxed) + 1;
                                        let current_failed = failed.load(Ordering::Relaxed);

                                        // Move to statistics line (first line), update it
                                        print!("\x1B[{}A", concurrent_limit + 1 - thread_id); // Move up to stats line
                                        print!("\x1B[2K\r{}", update_stats(current_completed, current_failed, total, *start_time));

                                        // Move to this thread's line, update it
                                        print!("\x1B[{}A", concurrent_limit - thread_id); // Move up to thread line
                                        print!("\x1B[2K\rThread {}: ✅ {} uploaded ({}/{})", 
                                                thread_id, track_id, current_completed + current_failed, total);
                                        print!("\x1B[{}B", concurrent_limit - thread_id); // Move back down
                                        io::stdout().flush().unwrap_or_default();

                                        // Mark as migrated in local JSON
                                        if let Err(e) =
                                            DjWizardLog::mark_track_as_migrated(&track_id)
                                        {
                                            eprintln!(
                                                "⚠️  Failed to mark track {} as migrated: {}",
                                                track_id, e
                                            );
                                        }

                                        return Ok(track_id);
                                    }
                                    Err(e) => {
                                        retry_count += 1;
                                        if retry_count > max_retries {
                                            let current_failed =
                                                failed.fetch_add(1, Ordering::Relaxed) + 1;
                                            let current_completed =
                                                completed.load(Ordering::Relaxed);

                                            // Move to statistics line (first line), update it
                                            print!("\x1B[{}A", concurrent_limit + 1 - thread_id); // Move up to stats line
                                            print!("\x1B[2K\r{}", update_stats(current_completed, current_failed, total, *start_time));

                                            // Move to this thread's line, update it
                                            print!("\x1B[{}A", concurrent_limit - thread_id); // Move up to thread line
                                            print!("\x1B[2K\rThread {}: ❌ {} failed: {} ({}/{})",
                                                    thread_id, track_id, e, current_completed + current_failed, total);
                                            print!("\x1B[{}B", concurrent_limit - thread_id); // Move back down
                                            io::stdout().flush().unwrap_or_default();

                                            return Err(e);
                                        }

                                        // Move to this thread's line, update it with retry info
                                        print!("\x1B[{}A", concurrent_limit - thread_id); // Move up to thread line
                                        print!("\x1B[2K\rThread {}: 🔄 {} retry {}/{} ({})", 
                                                thread_id, track_id, retry_count, max_retries, e);
                                        print!("\x1B[{}B", concurrent_limit - thread_id); // Move back down
                                        io::stdout().flush().unwrap_or_default();

                                        // Small delay before retry
                                        tokio::time::sleep(std::time::Duration::from_millis(
                                            100 * retry_count as u64,
                                        ))
                                        .await;
                                    }
                                }
                            }
                        })
                    };

                    let mut active_futures: FuturesUnordered<TrackFuture> = FuturesUnordered::new();
                    let mut next_track_idx = 0;

                    // Initialize statistics line and the 5 persistent thread status lines
                    print!("{}\n", update_stats(0, 0, total_tracks, start_time));
                    for i in 0..concurrent_limit {
                        print!("Thread {}: Waiting...\n", i);
                    }
                    // Move cursor to end to avoid interfering with updates
                    print!("\x1B[{};1H", concurrent_limit + 2);
                    io::stdout().flush().unwrap_or_default();

                    // Start initial concurrent uploads
                    while next_track_idx < total_tracks && active_futures.len() < concurrent_limit {
                        let (track_id, track) = tracks[next_track_idx].clone();
                        let thread_id = active_futures.len(); // Use current index as thread ID
                        let future = create_track_future(
                            track_id,
                            track,
                            firebase_client.clone(),
                            completed_count.clone(),
                            failed_count.clone(),
                            total_tracks,
                            thread_id,
                            start_time_shared.clone(),
                        );
                        active_futures.push(future);
                        next_track_idx += 1;
                    }

                    // Process completions and start new uploads immediately
                    while !active_futures.is_empty() {
                        if let Some(_result) = active_futures.next().await {
                            // Immediately start the next track if available
                            if next_track_idx < total_tracks {
                                let (track_id, track) = tracks[next_track_idx].clone();
                                let thread_id = next_track_idx % concurrent_limit; // Cycle through thread IDs
                                let future = create_track_future(
                                    track_id,
                                    track,
                                    firebase_client.clone(),
                                    completed_count.clone(),
                                    failed_count.clone(),
                                    total_tracks,
                                    thread_id,
                                    start_time_shared.clone(),
                                );
                                active_futures.push(future);
                                next_track_idx += 1;
                            }
                        }
                    }

                    // Final summary with timing
                    let final_completed = completed_count.load(Ordering::Relaxed);
                    let final_failed = failed_count.load(Ordering::Relaxed);
                    let total_processed = final_completed + final_failed;
                    let elapsed = start_time.elapsed();

                    println!("\n📊 Migration Complete!");
                    println!("   📈 Total processed: {} tracks", total_processed);
                    println!(
                        "   ✅ Successfully migrated: {} tracks ({}%)",
                        final_completed,
                        if total_processed > 0 {
                            (final_completed * 100) / total_processed
                        } else {
                            0
                        }
                    );
                    if final_failed > 0 {
                        println!(
                            "   ❌ Failed to migrate: {} tracks ({}%)",
                            final_failed,
                            if total_processed > 0 {
                                (final_failed * 100) / total_processed
                            } else {
                                0
                            }
                        );
                    }

                    // Timing statistics
                    println!("   ⏱️  Total time: {:.2?}", elapsed);
                    if final_completed > 0 {
                        let avg_time_per_track = elapsed / final_completed as u32;
                        println!("   📊 Average time per track: {:.2?}", avg_time_per_track);

                        let tracks_per_minute =
                            (final_completed as f64 / elapsed.as_secs_f64()) * 60.0;
                        println!(
                            "   🚀 Migration rate: {:.1} tracks/minute",
                            tracks_per_minute
                        );
                    }

                    if final_completed > 0 {
                        println!("🚀 Tracks are now accessible with O(1) performance by ID!");
                        println!("💡 Access any track instantly: firebase_client.get_track(\"track_id\").await");
                    }

                    if final_failed > 0 {
                        println!("⚠️  Some tracks failed to migrate. Check Firebase permissions and network connectivity.");
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
                individual_tracks,
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
