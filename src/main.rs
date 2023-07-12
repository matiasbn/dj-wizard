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
use crate::soundeo_log::SoundeoBotLog;
use crate::track::full_info::FullTrackInfo;
use crate::track::{SoundeoTrack, SoundeoTracksList};
use crate::user::{SoundeoUser, SoundeoUserConfig};

mod cleaner;
mod dialoguer;
mod errors;
mod soundeo_log;
mod track;
mod user;

#[derive(Debug)]
pub struct SoundeoBotError;
impl fmt::Display for SoundeoBotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Soundeo bot error")
    }
}
impl std::error::Error for SoundeoBotError {}

pub type SoundeoBotResult<T> = error_stack::Result<T, SoundeoBotError>;

#[derive(Parser, Debug, Clone)]
#[command(author, version, about = "Soundeo bot")]
struct Cli {
    #[command(subcommand)]
    command: SoundeoBotCommands,
}

/// A simple program to download all files from a search in soundeo
#[derive(Subcommand, Debug, PartialEq, Clone)]
enum SoundeoBotCommands {
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
}

impl SoundeoBotCommands {
    pub async fn execute(&self) -> SoundeoBotResult<()> {
        return match self {
            SoundeoBotCommands::Login => {
                let mut soundeo_user_config = SoundeoUserConfig::new();
                let prompt_text = format!("Soundeo user: ");
                soundeo_user_config.user =
                    Dialoguer::input(prompt_text).change_context(SoundeoBotError)?;
                let prompt_text = format!("Password: ");
                soundeo_user_config.pass =
                    Dialoguer::password(prompt_text).change_context(SoundeoBotError)?;
                let home_path = env::var("HOME")
                    .into_report()
                    .change_context(SoundeoBotError)?;
                let selected_path = FileDialog::new()
                    .set_location(&home_path)
                    .show_open_single_dir()
                    .into_report()
                    .change_context(SoundeoBotError)?
                    .ok_or(SoundeoBotError)
                    .into_report()?;
                soundeo_user_config.download_path = selected_path
                    .to_str()
                    .ok_or(SoundeoBotError)
                    .into_report()?
                    .to_string();
                println!(
                    "Soundeo credentials successfully stored:\n {:#?}",
                    soundeo_user_config
                );
                soundeo_user_config
                    .create_new_config_file()
                    .change_context(SoundeoBotError)?;
                Ok(())
            }
            SoundeoBotCommands::Config => {
                let mut soundeo_bot_config = SoundeoUserConfig::new();
                soundeo_bot_config
                    .read_config_file()
                    .change_context(SoundeoBotError)?;
                println!("Current config:\n{:#?}", soundeo_bot_config);
                Ok(())
            }
            SoundeoBotCommands::Queue => {
                let options = vec!["Add to queue", "Resume queue"];
                let prompt_text = "What you want to do?".to_string();
                let selection = Dialoguer::select(prompt_text, options, None)
                    .change_context(SoundeoBotError)?;
                if selection == 0 {
                    let prompt_text = format!("Soundeo url: ");
                    let url = Dialoguer::input(prompt_text).change_context(SoundeoBotError)?;
                    let soundeo_url = Url::parse(&url)
                        .into_report()
                        .change_context(SoundeoBotError)?;
                    let mut soundeo_user = SoundeoUser::new().change_context(SoundeoBotError)?;
                    soundeo_user
                        .login_and_update_user_info()
                        .await
                        .change_context(SoundeoBotError)?;
                    let mut track_list = SoundeoTracksList::new(soundeo_url.to_string())
                        .change_context(SoundeoBotError)?;
                    track_list
                        .get_tracks_id(&soundeo_user)
                        .await
                        .change_context(SoundeoBotError)?;
                    let mut soundeo_log =
                        SoundeoBotLog::read_log().change_context(SoundeoBotError)?;
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
                            .change_context(SoundeoBotError)?;
                        if queue_result {
                            println!(
                                "Track with id {} successfully queued",
                                track_id.clone().green(),
                            );
                            soundeo_log
                                .save_log(&soundeo_user)
                                .change_context(SoundeoBotError)?;
                        } else {
                            println!(
                                "Track with id {} was previously queued",
                                track_id.clone().red(),
                            );
                        }
                    }
                } else {
                    let mut soundeo_user = SoundeoUser::new().change_context(SoundeoBotError)?;
                    soundeo_user
                        .login_and_update_user_info()
                        .await
                        .change_context(SoundeoBotError)?;
                    let mut soundeo_log =
                        SoundeoBotLog::read_log().change_context(SoundeoBotError)?;
                    let queued_tracks = soundeo_log.queued_tracks.clone();
                    println!(
                        "The queue has {} tracks still pending to download",
                        format!("{}", queued_tracks.len()).cyan()
                    );
                    for track_id in queued_tracks {
                        soundeo_user
                            .validate_remaining_downloads()
                            .change_context(SoundeoBotError)?;
                        if soundeo_log.downloaded_tracks.contains_key(&track_id) {
                            println!("Track already downloaded: {}", track_id.clone());
                            continue;
                        }
                        let mut soundeo_track = SoundeoTrack::new(track_id.clone())
                            .await
                            .change_context(SoundeoBotError)?;
                        let download_result = soundeo_track
                            .download_track(&mut soundeo_user)
                            .await
                            .change_context(SoundeoBotError);
                        if let Ok(is_ok) = download_result {
                            soundeo_log
                                .remove_queued_track_from_log(track_id.clone())
                                .change_context(SoundeoBotError)?;
                            soundeo_log
                                .write_downloaded_track_to_log(soundeo_track.clone())
                                .change_context(SoundeoBotError)?;
                            soundeo_log
                                .save_log(&soundeo_user)
                                .change_context(SoundeoBotError)?;
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
            SoundeoBotCommands::Url => {
                let prompt_text = format!("Soundeo url: ");
                let url = Dialoguer::input(prompt_text).change_context(SoundeoBotError)?;
                let soundeo_url = Url::parse(&url)
                    .into_report()
                    .change_context(SoundeoBotError)?;
                let mut soundeo_user = SoundeoUser::new().change_context(SoundeoBotError)?;
                soundeo_user
                    .login_and_update_user_info()
                    .await
                    .change_context(SoundeoBotError)?;
                let mut track_list = SoundeoTracksList::new(soundeo_url.to_string())
                    .change_context(SoundeoBotError)?;
                track_list
                    .get_tracks_id(&soundeo_user)
                    .await
                    .change_context(SoundeoBotError)?;
                let mut soundeo_log = SoundeoBotLog::read_log().change_context(SoundeoBotError)?;
                for (track_id_index, track_id) in track_list.track_ids.iter().enumerate() {
                    // validate if we have can download tracks
                    soundeo_user
                        .validate_remaining_downloads()
                        .change_context(SoundeoBotError)?;
                    if soundeo_log.downloaded_tracks.contains_key(track_id) {
                        println!("Track already downloaded: {}", track_id.clone());
                        continue;
                    }
                    let mut soundeo_track = SoundeoTrack::new(track_id.clone())
                        .await
                        .change_context(SoundeoBotError)?;
                    let download_result = soundeo_track
                        .download_track(&mut soundeo_user)
                        .await
                        .change_context(SoundeoBotError);
                    if let Ok(is_ok) = download_result {
                        soundeo_log
                            .write_downloaded_track_to_log(soundeo_track.clone())
                            .change_context(SoundeoBotError)?;
                        soundeo_log
                            .save_log(&soundeo_user)
                            .change_context(SoundeoBotError)?;
                    } else {
                        println!(
                            "Track with id {} was not downloaded",
                            track_id.clone().red()
                        );
                    }
                }
                Ok(())
            }
            SoundeoBotCommands::Clean => {
                let soundeo_user = SoundeoUser::new().change_context(SoundeoBotError)?;
                println!("Select the folder to start cleaning repeated files");
                let selected_path = FileDialog::new()
                    .set_location(&soundeo_user.download_path)
                    .show_open_single_dir()
                    .into_report()
                    .change_context(SoundeoBotError)?
                    .ok_or(SoundeoBotError)
                    .into_report()?;
                println!(
                    "Cleaning {}",
                    selected_path.clone().to_str().unwrap().cyan()
                );
                clean_repeated_files(selected_path).change_context(SoundeoBotError)
            }
            SoundeoBotCommands::Info => {
                let prompt_text = "Soundeo track id: ".to_string();
                let track_id = Dialoguer::input(prompt_text).change_context(SoundeoBotError)?;
                let mut soundeo_user = SoundeoUser::new().change_context(SoundeoBotError)?;
                soundeo_user
                    .login_and_update_user_info()
                    .await
                    .change_context(SoundeoBotError)?;
                let mut soundeo_track_full_info = FullTrackInfo::new(track_id);
                soundeo_track_full_info
                    .get_track_info(&soundeo_user)
                    .await
                    .change_context(SoundeoBotError)?;
                println!("{:#?}", soundeo_track_full_info);
                Ok(())
            }
        };
    }

    pub fn cli_command(&self) -> String {
        match self {
            SoundeoBotCommands::Login => {
                format!("soundeo-bot login")
            }
            SoundeoBotCommands::Config => {
                format!("soundeo-bot config")
            }
            SoundeoBotCommands::Queue => {
                format!("soundeo-bot queue")
            }
            SoundeoBotCommands::Url => {
                format!("soundeo-bot url")
            }
            SoundeoBotCommands::Clean => {
                format!("soundeo-bot clean")
            }
            SoundeoBotCommands::Info => {
                format!("soundeo-bot clean")
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

async fn run() -> SoundeoBotResult<()> {
    let cli = Cli::parse();

    Suggestion::set_report();

    cli.command.execute().await?;

    // Ok(())
    Ok(())
}

#[tokio::main]
async fn main() -> SoundeoBotResult<()> {
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
