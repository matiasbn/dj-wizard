use std::{env, fmt};

use clap::{Parser, Subcommand};
use colored::Colorize;
use error_stack::fmt::{Charset, ColorMode};
use error_stack::{FutureExt, IntoReport, Report, ResultExt};
use native_dialog::FileDialog;

use crate::backup::commands::BackupCommands;
use crate::cleaner::clean_repeated_files;
use crate::dialoguer::Dialoguer;
use crate::log::DjWizardLog;
use crate::queue::commands::QueueCommands;
use crate::soundeo::track::SoundeoTrack;
use crate::spotify::commands::SpotifyCommands;
use crate::url_list::commands::UrlListCommands;
use crate::user::{SoundeoUser, User};

mod backup;
mod cleaner;
mod config;
mod dialoguer;
mod errors;
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
    Spotify,
    /// Backup the log file to the cloud
    Backup,
}

impl DjWizardCommands {
    pub async fn execute(&self) -> DjWizardResult<()> {
        return match self {
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
            DjWizardCommands::Spotify => {
                SpotifyCommands::execute()
                    .change_context(DjWizardError)
                    .await
            }
            DjWizardCommands::Backup => {
                BackupCommands::execute()
                    .change_context(DjWizardError)
                    .await
            }
        };
    }

    pub fn cli_command(&self) -> String {
        match self {
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
                format!("dj-wizard clean")
            }
            DjWizardCommands::Spotify => {
                format!("dj-wizard spotify")
            }
            DjWizardCommands::IPFS => {
                format!("dj-wizard ipfs")
            }
            DjWizardCommands::Backup => {
                format!("dj-wizard backup")
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
