use std::collections::HashMap;
use std::{env, fmt};

use crate::cleaner::clean_repeated_files;
use clap::{Parser, Subcommand};
use colored::Colorize;
use error_stack::fmt::{Charset, ColorMode};
use error_stack::{FutureExt, IntoReport, Report, ResultExt};
use native_dialog::FileDialog;
use reqwest::{get, Client};
use scraper::{ElementRef, Html, Selector};
use serde_json::json;
use url::{Host, Position, Url};

use crate::dialoguer::Dialoguer;
use crate::soundeo::full_info::SoundeoTrackFullInfo;
use crate::soundeo::track::{SoundeoTrack, SoundeoTracksList};
use crate::soundeo_log::DjWizardLog;
use crate::spotify::playlist::SpotifyPlaylist;
use crate::spotify::SpotifyCommands;
use crate::user::{SoundeoUser, SoundeoUserConfig};

mod cleaner;
mod dialoguer;
mod errors;
mod soundeo;
mod soundeo_log;
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
                let mut soundeo_user_config = SoundeoUserConfig::new();
                let prompt_text = format!("Soundeo user: ");
                soundeo_user_config.user =
                    Dialoguer::input(prompt_text).change_context(DjWizardError)?;
                let prompt_text = format!("Password: ");
                soundeo_user_config.pass =
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
            DjWizardCommands::Config => {
                let mut soundeo_bot_config = SoundeoUserConfig::new();
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
                        if soundeo_log.downloaded_tracks.contains_key(track_id) {
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
                            soundeo_log
                                .save_log(&soundeo_user)
                                .change_context(DjWizardError)?;
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
                    let mut soundeo_log = DjWizardLog::read_log().change_context(DjWizardError)?;
                    let queued_tracks = soundeo_log.queued_tracks.clone();
                    println!(
                        "The queue has {} tracks still pending to download",
                        format!("{}", queued_tracks.len()).cyan()
                    );
                    for track_id in queued_tracks {
                        soundeo_user
                            .validate_remaining_downloads()
                            .change_context(DjWizardError)?;
                        if soundeo_log.downloaded_tracks.contains_key(&track_id) {
                            println!("Track already downloaded: {}", track_id.clone());
                            continue;
                        }
                        let mut soundeo_track = SoundeoTrack::new(track_id.clone())
                            .await
                            .change_context(DjWizardError)?;
                        let download_result = soundeo_track
                            .download_track(&mut soundeo_user)
                            .await
                            .change_context(DjWizardError);
                        if let Ok(is_ok) = download_result {
                            soundeo_log
                                .remove_queued_track_from_log(track_id.clone())
                                .change_context(DjWizardError)?;
                            soundeo_log
                                .write_downloaded_track_to_log(soundeo_track.clone())
                                .change_context(DjWizardError)?;
                            soundeo_log
                                .save_log(&soundeo_user)
                                .change_context(DjWizardError)?;
                        } else {
                            println!(
                                "Track with id {} was not downloaded",
                                track_id.clone().red()
                            );
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
                let mut soundeo_log = DjWizardLog::read_log().change_context(DjWizardError)?;
                for (track_id_index, track_id) in track_list.track_ids.iter().enumerate() {
                    // validate if we have can download tracks
                    soundeo_user
                        .validate_remaining_downloads()
                        .change_context(DjWizardError)?;
                    if soundeo_log.downloaded_tracks.contains_key(track_id) {
                        println!("Track already downloaded: {}", track_id.clone());
                        continue;
                    }
                    let mut soundeo_track = SoundeoTrack::new(track_id.clone())
                        .await
                        .change_context(DjWizardError)?;
                    let download_result = soundeo_track
                        .download_track(&mut soundeo_user)
                        .await
                        .change_context(DjWizardError);
                    if let Ok(is_ok) = download_result {
                        soundeo_log
                            .write_downloaded_track_to_log(soundeo_track.clone())
                            .change_context(DjWizardError)?;
                        soundeo_log
                            .save_log(&soundeo_user)
                            .change_context(DjWizardError)?;
                    } else {
                        println!(
                            "Track with id {} was not downloaded",
                            track_id.clone().red()
                        );
                    }
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
                let mut soundeo_track_full_info = SoundeoTrackFullInfo::new(track_id);
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
    // let query_parameters: HashMap<_, _> = soundeo_url.query_pairs().into_owned().collect();
    // println!("{:#?}", query_parameters);
    // let retrieved_page = retrieve_html(soundeo_url.to_string());
    // let page_body = Html::parse_document(&retrieved_page);
    // let songs_class = Selector::parse(".trackitem").unwrap();
    // let mut songs = page_body.select(&songs_class);
    // let song = songs.next().unwrap();
    // let track_id = get_track_id(song);
    // println!("{:#?}", track_id);
    // for song in page_body.select(&songs_class) {
    //     let song_title: Vec<_> = song.text().collect();
    //     println!("{:#?}", song_title);
    // }
}
