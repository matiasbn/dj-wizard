use std::collections::HashMap;
use std::process::Termination;
use std::{env, fmt};

use clap::{Parser, Subcommand};
use colored::Colorize;
use error_stack::fmt::{Charset, ColorMode};
use error_stack::{FutureExt, IntoReport, Report, ResultExt};
use native_dialog::FileDialog;
use reqwest::{get, Client};
use scraper::{ElementRef, Html, Selector};
use serde_json::json;
use url::{Host, Position, Url};

use crate::cleaner::clean_repeated_files;
use crate::dialoguer::Dialoguer;
use crate::log::DjWizardLog;
use crate::soundeo::track::SoundeoTrack;
use crate::soundeo::track_list::SoundeoTracksList;
use crate::spotify::commands::SpotifyCommands;
use crate::spotify::playlist::SpotifyPlaylist;
use crate::user::{SoundeoUser, User};

mod cleaner;
mod dialoguer;
mod errors;
mod ipfs;
mod log;
mod soundeo;
mod spotify;
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
    Queue,
    /// Downloads the tracks from a Soundeo url
    Url,
    /// Clean all the repeated files starting on a path.
    /// Thought to be used to clean repeated files
    /// on DJ programs e.g. Rekordbox
    Clean,
    /// Get Soundeo track info by id
    Info,
    /// Automatically download tracks from a Spotify playlist
    Spotify,
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
                    let log = DjWizardLog::read_log().change_context(DjWizardError)?;
                    log.upload_to_ipfs().change_context(DjWizardError)?;
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
            DjWizardCommands::Queue => {
                let options = vec!["Add to queue", "Resume queue"];
                let prompt_text = "What you want to do?".to_string();
                let selection =
                    Dialoguer::select(prompt_text, options, None).change_context(DjWizardError)?;
                if selection == 0 {
                    let prompt_text = format!("Soundeo url: ");
                    let url = Dialoguer::input(prompt_text).change_context(DjWizardError)?;
                    let soundeo_url = Url::parse(&url)
                        .into_report()
                        .change_context(DjWizardError)?;
                    let mut soundeo_user = SoundeoUser::new().change_context(DjWizardError)?;
                    soundeo_user
                        .login_and_update_user_info()
                        .await
                        .change_context(DjWizardError)?;
                    let mut track_list = SoundeoTracksList::new(soundeo_url.to_string())
                        .change_context(DjWizardError)?;
                    track_list
                        .get_tracks_id(&soundeo_user)
                        .await
                        .change_context(DjWizardError)?;
                    let mut soundeo_log = DjWizardLog::read_log().change_context(DjWizardError)?;
                    println!(
                        "Queueing {} tracks",
                        format!("{}", track_list.track_ids.len()).cyan()
                    );
                    for (track_id_index, track_id) in track_list.track_ids.iter().enumerate() {
                        let mut track_info = SoundeoTrack::new(track_id.clone());
                        track_info
                            .get_info(&soundeo_user)
                            .await
                            .change_context(DjWizardError)?;
                        if track_info.already_downloaded {
                            println!("Track already downloaded: {}", track_id.clone().yellow());
                            continue;
                        }
                        println!(
                            "Queueing track with id {}, {} of {}",
                            track_id.clone().cyan(),
                            track_id_index + 1,
                            track_list.track_ids.len()
                        );
                        let queue_result = soundeo_log
                            .write_queued_track_to_log(track_id.clone())
                            .change_context(DjWizardError)?;
                        if queue_result {
                            println!(
                                "Track with id {} successfully queued",
                                track_id.clone().green(),
                            );
                        } else {
                            println!(
                                "Track with id {} was previously queued",
                                track_id.clone().red(),
                            );
                        }
                    }
                } else {
                    let mut soundeo_user = SoundeoUser::new().change_context(DjWizardError)?;
                    soundeo_user
                        .login_and_update_user_info()
                        .await
                        .change_context(DjWizardError)?;
                    let queued_tracks =
                        DjWizardLog::get_queued_tracks().change_context(DjWizardError)?;
                    println!(
                        "The queue has {} tracks still pending to download",
                        format!("{}", queued_tracks.len()).cyan()
                    );
                    for track_id in queued_tracks {
                        let mut track_info = SoundeoTrack::new(track_id.clone());
                        let download_result = track_info
                            .download_track(&mut soundeo_user)
                            .await
                            .change_context(DjWizardError);
                        match download_result {
                            Ok(_) => {
                                DjWizardLog::remove_queued_track_from_log(track_id.clone())
                                    .change_context(DjWizardError)?;
                            }
                            Err(error) => {
                                println!(
                                    "Track with id {} was not downloaded",
                                    track_id.clone().red()
                                );
                                println!("Error: {:?}", error)
                            }
                        }
                    }
                }
                Ok(())
            }
            DjWizardCommands::Url => {
                let prompt_text = format!("Soundeo url: ");
                let url = Dialoguer::input(prompt_text).change_context(DjWizardError)?;
                let soundeo_url = Url::parse(&url)
                    .into_report()
                    .change_context(DjWizardError)?;
                let mut soundeo_user = SoundeoUser::new().change_context(DjWizardError)?;
                soundeo_user
                    .login_and_update_user_info()
                    .await
                    .change_context(DjWizardError)?;
                let mut track_list = SoundeoTracksList::new(soundeo_url.to_string())
                    .change_context(DjWizardError)?;
                track_list
                    .get_tracks_id(&soundeo_user)
                    .await
                    .change_context(DjWizardError)?;
                for (_, track_id) in track_list.track_ids.into_iter().enumerate() {
                    let mut track = SoundeoTrack::new(track_id);
                    track
                        .download_track(&mut soundeo_user)
                        .await
                        .change_context(DjWizardError)?;
                }
                Ok(())
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
                    .get_info(&soundeo_user)
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
            DjWizardCommands::Queue => {
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
