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
                        println!("Authentication successful! Your data will now sync to the cloud.");
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
            DjWizardCommands::Artist => {
                ArtistCommands::execute()
                    .change_context(DjWizardError)
            }
            DjWizardCommands::Migrate { soundeo_log, light_only } => {
                use crate::auth::google_auth::GoogleAuth;
                use crate::auth::firebase_client::FirebaseClient;
                
                // Try to load existing token
                let auth_token = match GoogleAuth::load_token() {
                    Ok(token) => token,
                    Err(_) => {
                        println!("❌ No valid authentication found. Please run 'dj-wizard auth' first.");
                        return Err(DjWizardError).into_report();
                    }
                };
                
                // Create Firebase client
                let firebase_client = FirebaseClient::new(auth_token).await
                    .change_context(DjWizardError)?;
                
                // Set default path if not provided
                let default_log_path = "/Users/matiasbn/soundeo-bot-files/soundeo_log.json";
                let log_path = soundeo_log.as_deref().unwrap_or(default_log_path);
                
                println!("🔄 Migrating complete soundeo_log.json to Firebase...");
                println!("📂 Reading from: {}", log_path);
                
                // Check file size first
                let metadata = std::fs::metadata(log_path)
                    .map_err(|e| {
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
                let log_data = std::fs::read_to_string(log_path)
                    .map_err(|e| {
                        println!("❌ Failed to read log file: {}", e);
                        DjWizardError
                    })?;
                
                println!("✅ File read successfully ({} bytes)", log_data.len());
                
                // Parse as JSON to validate it's correct
                println!("🔍 Parsing JSON...");
                let json_value: serde_json::Value = serde_json::from_str(&log_data)
                    .map_err(|e| {
                        println!("❌ Invalid JSON file: {}", e);
                        DjWizardError
                    })?;
                
                println!("✅ JSON parsed successfully");
                
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
                println!("📊 Final upload size: {:.2} MB ({} bytes)", final_size_mb, final_size);
                
                // Upload the data
                println!("☁️  Uploading to Firebase...");
                firebase_client.set_document("dj_wizard_data", "soundeo_log", &final_data).await
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
            DjWizardCommands::Migrate { soundeo_log, light_only } => {
                let mut cmd = "dj-wizard migrate".to_string();
                if let Some(log_path) = soundeo_log {
                    cmd.push_str(&format!(" --soundeo-log {}", log_path));
                }
                if *light_only {
                    cmd.push_str(" --light-only");
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
