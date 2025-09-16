use clap::Parser;
use error_stack::{IntoReport, ResultExt};

use crate::migrate::{MigrateError, MigrateResult};

#[derive(Parser, Debug, Clone, PartialEq)]
pub struct MigrateCli {
    /// Path to soundeo_log.json file
    #[clap(long)]
    pub soundeo_log: Option<String>,
    /// Only migrate light fields (exclude soundeo and queued_tracks)
    #[clap(long)]
    pub light_only: bool,
    /// Migrate queued tracks
    #[clap(long)]
    pub queued_tracks: bool,
    /// Migrate soundeo field
    #[clap(long)]
    pub soundeo: bool,
    /// Migrate all remaining fields (tracks_info, etc.)
    #[clap(long)]
    pub remaining: bool,
    /// Migrate tracks as individual documents for O(1) access (super fast)
    #[clap(long)]
    pub individual_tracks: bool,
    /// Migrate queue to priority-based structure
    #[clap(long)]
    pub queue: bool,
}

impl MigrateCli {

    pub async fn execute(&self) -> MigrateResult<()> {
        let Self {
            soundeo_log,
            light_only,
            queued_tracks,
            soundeo,
            remaining,
            individual_tracks,
            queue,
        } = self;
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
                        .change_context(MigrateError)?
                } else {
                    token
                }
            }
            Err(_) => {
                println!("❌ No valid authentication found. Please run 'dj-wizard auth' first.");
                return Err(MigrateError).into_report();
            }
        };

        // Create Firebase client
        let mut firebase_client = FirebaseClient::new(auth_token)
            .await
            .change_context(MigrateError)?;

        // Set default path if not provided
        let default_log_path = "/Users/matiasbn/soundeo-bot-files/soundeo_log.json";
        let log_path = soundeo_log.as_deref().unwrap_or(default_log_path);

        println!("🔄 Migrating complete soundeo_log.json to Firebase...");
        println!("📂 Reading from: {}", log_path);

        // Check file size first
        let metadata = std::fs::metadata(log_path).map_err(|e| {
            println!("❌ Failed to read file metadata: {}", e);
            MigrateError
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
            MigrateError
        })?;

        println!("✅ File read successfully ({} bytes)", log_data.len());

        // Parse as JSON to validate it's correct
        println!("🔍 Parsing JSON...");
        let json_value: serde_json::Value = serde_json::from_str(&log_data).map_err(|e| {
            println!("❌ Invalid JSON file: {}", e);
            MigrateError
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
                        .change_context(MigrateError)?
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
                        .change_context(MigrateError)?;

                    println!(
                        "🎉 Successfully added {} queued tracks to existing document!",
                        tracks.len()
                    );
                    return Ok(());
                } else {
                    println!("❌ No 'queued_tracks' field found in JSON");
                    return Err(MigrateError).into_report();
                }
            } else {
                println!("❌ JSON is not an object");
                return Err(MigrateError).into_report();
            }
        }

        if *soundeo {
            // Ultra fast individual track migration with O(1) access using same parallelism
            println!(
                "⚡ Soundeo mode: Migrating tracks as individual documents for O(1) access..."
            );

            // Read soundeo using the existing working logic
            use crate::log::DjWizardLog;
            let current_soundeo = DjWizardLog::get_soundeo().change_context(MigrateError)?;

            println!(
                "📊 Found soundeo with {} tracks_info for ultra-fast individual migration",
                current_soundeo.tracks_info.len()
            );

            if current_soundeo.tracks_info.is_empty() {
                println!("ℹ️  No tracks to migrate in soundeo mode");
                return Ok(());
            }

            // STEP 1: Get existing tracks from Firebase and save them in bulk
            println!("🔍 Checking existing tracks in Firebase...");
            let existing_ids = firebase_client
                .get_all_firebase_track_ids()
                .await
                .unwrap_or_default();
            println!(
                "📊 Found {} existing tracks in Firebase",
                existing_ids.len()
            );

            if !existing_ids.is_empty() {
                println!(
                    "📝 Saving {} existing track IDs to soundeo_log.json in bulk...",
                    existing_ids.len()
                );
                let _ = DjWizardLog::set_firebase_migrated_tracks(existing_ids.clone());
                println!("✅ Saved existing Firebase track IDs in bulk");
            }

            // STEP 2: Reload and filter only tracks NOT in Firebase using bulk array
            let current_soundeo_refreshed =
                DjWizardLog::get_soundeo().change_context(MigrateError)?;
            let firebase_migrated =
                DjWizardLog::get_firebase_migrated_tracks().change_context(MigrateError)?;
            let firebase_migrated_set: std::collections::HashSet<String> =
                firebase_migrated.into_iter().collect();
            let total_all_tracks = current_soundeo_refreshed.tracks_info.len();

            let tracks_to_migrate: Vec<_> = current_soundeo_refreshed
                .tracks_info
                .into_iter()
                .filter(|(track_id, track)| {
                    // Only migrate if NOT migrated AND NOT in Firebase migrated array
                    !track.migrated && !firebase_migrated_set.contains(track_id)
                })
                .collect();

            let total_tracks = tracks_to_migrate.len();
            let already_migrated = total_all_tracks - total_tracks;

            println!(
                "📊 Found {} total tracks, {} already migrated, {} pending migration",
                total_all_tracks, already_migrated, total_tracks
            );

            if total_tracks == 0 {
                println!("✅ All tracks already exist in Firebase or are marked as migrated!");
                return Ok(());
            }

            let tracks = tracks_to_migrate;
            let batch_size = 20; // Conservative batch size for reliability
            let concurrent_batches = 5; // 3 concurrent batch threads
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
                    return Err(MigrateError).into_report();
                }
            }

            // Split tracks into batches of 200
            let batches: Vec<_> = tracks.chunks(batch_size).collect();
            let total_batches = batches.len();

            println!(
                "🚀 Starting batch migration: {} batches of {} tracks with {} concurrent threads",
                total_batches, batch_size, concurrent_batches
            );

            // Migration timing
            let start_time = std::time::Instant::now();

            // Helper function to update real-time stats
            let update_stats =
                |completed: usize, failed: usize, total: usize, start_time: std::time::Instant| {
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

                    let eta = if rate > 0.0 {
                        let remaining_tracks = total - processed;
                        let minutes_remaining = remaining_tracks as f64 / rate;
                        if minutes_remaining < 60.0 {
                            format!("{:.0}m", minutes_remaining)
                        } else {
                            let hours = minutes_remaining / 60.0;
                            format!("{:.1}h", hours)
                        }
                    } else {
                        "∞".to_string()
                    };

                    format!(
                        "📊 {}/{} | ✅ {} ❌ {} | ⏱️ {:?} | 📈 {:.2?}/track | 🚀 {:.1}/min | ⏳ {}",
                        processed, total, completed, failed, elapsed, avg_time, rate, eta
                    )
                };

            // Progress tracking
            use std::sync::atomic::{AtomicUsize, Ordering};
            use std::sync::Arc;
            let completed_count = Arc::new(AtomicUsize::new(0));
            let failed_count = Arc::new(AtomicUsize::new(0));
            let firebase_client = Arc::new(firebase_client);

            // Initialize status display
            println!("{}", update_stats(0, 0, total_tracks, start_time));
            for i in 0..concurrent_batches {
                println!("Batch Thread {}: Waiting...", i);
            }

            // Spawn a task to update statistics every 500ms
            let stats_completed = completed_count.clone();
            let stats_failed = failed_count.clone();
            let stats_update = tokio::spawn(async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    let current_completed = stats_completed.load(Ordering::Relaxed);
                    let current_failed = stats_failed.load(Ordering::Relaxed);

                    // Update statistics line (first line)
                    print!("\x1B[{}A", concurrent_batches + 1); // Move up to stats line
                    print!(
                        "\x1B[2K\r{}",
                        update_stats(current_completed, current_failed, total_tracks, start_time)
                    );
                    print!("\x1B[{}B", concurrent_batches + 1); // Move back down
                    std::io::Write::flush(&mut std::io::stdout()).unwrap_or_default();

                    // Stop when migration is complete
                    if current_completed + current_failed >= total_tracks {
                        break;
                    }
                }
            });

            // Process batches concurrently using tokio tasks
            use futures_util::stream::{FuturesUnordered, StreamExt};
            let mut batch_futures = FuturesUnordered::new();
            let mut batch_iterator = batches.into_iter().enumerate();

            // Start initial concurrent batches
            for _ in 0..concurrent_batches.min(total_batches) {
                if let Some((batch_idx, batch)) = batch_iterator.next() {
                    let thread_id = batch_idx % concurrent_batches;
                    let client = firebase_client.clone();
                    let completed = completed_count.clone();
                    let failed = failed_count.clone();
                    let batch_tracks: Vec<_> = batch.iter().cloned().collect();

                    let future = tokio::spawn(async move {
                        let mut retry_count = 0;

                        loop {
                            // Update status
                            print!("\x1B[{}A", concurrent_batches - thread_id);
                            print!(
                                "\x1B[2K\rBatch Thread {}: Processing batch {} ({} tracks)...",
                                thread_id,
                                batch_idx + 1,
                                batch_tracks.len()
                            );
                            print!("\x1B[{}B", concurrent_batches - thread_id);
                            std::io::Write::flush(&mut std::io::stdout()).unwrap_or_default();

                            match client.batch_write_tracks(&batch_tracks).await {
                                Ok(_) => {
                                    // Mark all tracks in batch as migrated
                                    for (track_id, _) in &batch_tracks {
                                        let _ = DjWizardLog::mark_track_as_migrated(track_id);
                                    }

                                    let current_completed = completed
                                        .fetch_add(batch_tracks.len(), Ordering::Relaxed)
                                        + batch_tracks.len();

                                    // Update status
                                    print!("\x1B[{}A", concurrent_batches - thread_id);
                                    print!("\x1B[2K\rBatch Thread {}: ✅ Batch {} completed ({} tracks)", 
                                                    thread_id, batch_idx + 1, batch_tracks.len());
                                    print!("\x1B[{}B", concurrent_batches - thread_id);
                                    std::io::Write::flush(&mut std::io::stdout())
                                        .unwrap_or_default();

                                    return Ok((thread_id, batch_tracks.len()));
                                }
                                Err(e) => {
                                    retry_count += 1;
                                    if retry_count > max_retries {
                                        let current_failed = failed
                                            .fetch_add(batch_tracks.len(), Ordering::Relaxed)
                                            + batch_tracks.len();

                                        // Update status
                                        print!("\x1B[{}A", concurrent_batches - thread_id);
                                        print!(
                                            "\x1B[2K\rBatch Thread {}: ❌ Batch {} failed: {}",
                                            thread_id,
                                            batch_idx + 1,
                                            e
                                        );
                                        print!("\x1B[{}B", concurrent_batches - thread_id);
                                        std::io::Write::flush(&mut std::io::stdout())
                                            .unwrap_or_default();

                                        return Err(e);
                                    }

                                    // Update retry status
                                    print!("\x1B[{}A", concurrent_batches - thread_id);
                                    print!(
                                        "\x1B[2K\rBatch Thread {}: 🔄 Batch {} retry {}/{} ({})",
                                        thread_id,
                                        batch_idx + 1,
                                        retry_count,
                                        max_retries,
                                        e
                                    );
                                    print!("\x1B[{}B", concurrent_batches - thread_id);
                                    std::io::Write::flush(&mut std::io::stdout())
                                        .unwrap_or_default();

                                    // Small delay before retry
                                    tokio::time::sleep(std::time::Duration::from_millis(
                                        1000 * retry_count as u64,
                                    ))
                                    .await;
                                }
                            }
                        }
                    });

                    batch_futures.push(future);
                }
            }

            // Process remaining batches as current ones complete
            while !batch_futures.is_empty() {
                if let Some(result) = batch_futures.next().await {
                    // A batch completed, start the next one if available
                    if let Some((batch_idx, batch)) = batch_iterator.next() {
                        let thread_id = if let Ok(Ok((completed_thread_id, _))) = result {
                            completed_thread_id // Reuse the thread ID that just completed
                        } else {
                            batch_idx % concurrent_batches // Fallback
                        };

                        let client = firebase_client.clone();
                        let completed = completed_count.clone();
                        let failed = failed_count.clone();
                        let batch_tracks: Vec<_> = batch.iter().cloned().collect();

                        let future = tokio::spawn(async move {
                            let mut retry_count = 0;

                            loop {
                                // Update status
                                print!("\x1B[{}A", concurrent_batches - thread_id);
                                print!(
                                    "\x1B[2K\rBatch Thread {}: Processing batch {} ({} tracks)...",
                                    thread_id,
                                    batch_idx + 1,
                                    batch_tracks.len()
                                );
                                print!("\x1B[{}B", concurrent_batches - thread_id);
                                std::io::Write::flush(&mut std::io::stdout()).unwrap_or_default();

                                match client.batch_write_tracks(&batch_tracks).await {
                                    Ok(_) => {
                                        // Mark all tracks in batch as migrated
                                        for (track_id, _) in &batch_tracks {
                                            let _ = DjWizardLog::mark_track_as_migrated(track_id);
                                        }

                                        let current_completed = completed
                                            .fetch_add(batch_tracks.len(), Ordering::Relaxed)
                                            + batch_tracks.len();

                                        // Update status
                                        print!("\x1B[{}A", concurrent_batches - thread_id);
                                        print!("\x1B[2K\rBatch Thread {}: ✅ Batch {} completed ({} tracks)", 
                                                        thread_id, batch_idx + 1, batch_tracks.len());
                                        print!("\x1B[{}B", concurrent_batches - thread_id);
                                        std::io::Write::flush(&mut std::io::stdout())
                                            .unwrap_or_default();

                                        return Ok((thread_id, batch_tracks.len()));
                                    }
                                    Err(e) => {
                                        retry_count += 1;
                                        if retry_count > max_retries {
                                            let current_failed = failed
                                                .fetch_add(batch_tracks.len(), Ordering::Relaxed)
                                                + batch_tracks.len();

                                            // Update status
                                            print!("\x1B[{}A", concurrent_batches - thread_id);
                                            print!(
                                                "\x1B[2K\rBatch Thread {}: ❌ Batch {} failed: {}",
                                                thread_id,
                                                batch_idx + 1,
                                                e
                                            );
                                            print!("\x1B[{}B", concurrent_batches - thread_id);
                                            std::io::Write::flush(&mut std::io::stdout())
                                                .unwrap_or_default();

                                            return Err(e);
                                        }

                                        // Update retry status
                                        print!("\x1B[{}A", concurrent_batches - thread_id);
                                        print!("\x1B[2K\rBatch Thread {}: 🔄 Batch {} retry {}/{} ({})", 
                                                        thread_id, batch_idx + 1, retry_count, max_retries, e);
                                        print!("\x1B[{}B", concurrent_batches - thread_id);
                                        std::io::Write::flush(&mut std::io::stdout())
                                            .unwrap_or_default();

                                        // Small delay before retry
                                        tokio::time::sleep(std::time::Duration::from_millis(
                                            1000 * retry_count as u64,
                                        ))
                                        .await;
                                    }
                                }
                            }
                        });

                        batch_futures.push(future);
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

                let tracks_per_minute = (final_completed as f64 / elapsed.as_secs_f64()) * 60.0;
                println!(
                    "   🚀 Migration rate: {:.1} tracks/minute",
                    tracks_per_minute
                );
            }

            if final_completed > 0 {
                println!("🚀 Tracks are now accessible with O(1) performance by ID!");
                println!(
                    "💡 Access any track instantly: firebase_client.get_track(\"track_id\").await"
                );
            }

            if final_failed > 0 {
                println!("⚠️  Some tracks failed to migrate. Check Firebase permissions and network connectivity.");
            }

            // Cancel the statistics update task
            stats_update.abort();

            return Ok(());
        }

        if *queue {
            // Queue migration mode
            println!("📋 Queue mode: Migrating to priority-based structure...");

            // Read current queue from DjWizardLog
            use crate::log::DjWizardLog;
            let all_queued_tracks =
                DjWizardLog::get_queued_tracks().change_context(MigrateError)?;

            // Get Firebase migrated queue IDs for bulk filtering (much faster than individual checks)
            let firebase_migrated_queues =
                DjWizardLog::get_firebase_migrated_queues().change_context(MigrateError)?;
            let firebase_migrated_set: std::collections::HashSet<String> =
                firebase_migrated_queues.into_iter().collect();
            let total_all_queued_tracks = all_queued_tracks.len();

            // Filter only tracks NOT migrated AND NOT in Firebase migrated array (bulk filtering)
            let queued_tracks: Vec<_> = all_queued_tracks
                .into_iter()
                .filter(|track| {
                    // Only migrate if NOT migrated AND NOT in Firebase migrated array
                    !track.migrated && !firebase_migrated_set.contains(&track.track_id)
                })
                .collect();

            let already_migrated = total_all_queued_tracks - queued_tracks.len();

            if queued_tracks.is_empty() {
                println!(
                            "ℹ️  No pending queued tracks found to migrate ({} already processed using bulk filtering)",
                            already_migrated
                        );
                return Ok(());
            }

            println!(
                "📊 Found {} pending queued tracks to migrate ({} already processed, {} total)",
                queued_tracks.len(),
                already_migrated,
                total_all_queued_tracks
            );

            // Migrate to priority-based structure
            firebase_client
                .migrate_queue_to_subcollections(&queued_tracks)
                .await
                .change_context(MigrateError)?;

            println!("🎉 Queue successfully migrated to priority-based subcollections!");
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
                    .change_context(MigrateError)?
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
                    println!("ℹ️  No new fields to add - document already contains all data");
                    return Ok(());
                }

                println!("✅ Added fields: {:?}", added_fields);

                // Upload updated document
                println!("☁️  Uploading updated document...");
                firebase_client
                    .set_document("dj_wizard_data", "soundeo_log", &existing_doc)
                    .await
                    .change_context(MigrateError)?;

                println!(
                    "🎉 Successfully added {} remaining fields to existing document!",
                    added_fields.len()
                );
                return Ok(());
            } else {
                println!("❌ JSON is not an object");
                return Err(MigrateError).into_report();
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
            .change_context(MigrateError)?;

        println!("✅ Successfully migrated entire soundeo_log.json to Firebase!");
        println!("🎉 Your data is now available in the cloud!");
        Ok(())
    }

    pub fn to_cli_command(&self) -> String {
        let mut cmd = "dj-wizard migrate".to_string();
        if let Some(log_path) = &self.soundeo_log {
            cmd.push_str(&format!(" --soundeo-log {}", log_path));
        }
        if self.light_only {
            cmd.push_str(" --light-only");
        }
        if self.queued_tracks {
            cmd.push_str(" --queued-tracks");
        }
        if self.soundeo {
            cmd.push_str(" --soundeo");
        }
        if self.remaining {
            cmd.push_str(" --remaining");
        }
        if self.individual_tracks {
            cmd.push_str(" --individual-tracks");
        }
        if self.queue {
            cmd.push_str(" --queue");
        }
        cmd
    }
}
