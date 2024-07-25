use clap::builder::Str;
use colored::Colorize;
use error_stack::{FutureExt, IntoReport, Report, ResultExt};
use inflector::Inflector;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use strum::IntoEnumIterator;
use url::Url;

use crate::dialoguer::Dialoguer;
use crate::log::DjWizardLog;
use crate::queue::{QueueError, QueueResult};
use crate::soundeo::track::SoundeoTrack;
use crate::soundeo::track_list::SoundeoTracksList;
use crate::soundeo::Soundeo;
use crate::spotify::playlist::SpotifyPlaylist;
use crate::spotify::SpotifyError;
use crate::url_list::UrlListCRUD;
use crate::user::SoundeoUser;

#[derive(Debug, Deserialize, Serialize, Clone, strum_macros::Display, strum_macros::EnumIter)]
pub enum QueueCommands {
    AddToQueueFromUrl,
    AddToQueueFromUrlList,
    ResumeQueue,
    SaveToAvailableTracks,
    DownloadOnlyAvailableTracks,
}

impl QueueCommands {
    pub async fn execute() -> QueueResult<()> {
        let options = Self::get_options();
        let selection = Dialoguer::select("What you want to do?".to_string(), options, None)
            .change_context(QueueError)?;
        return match Self::get_selection(selection) {
            QueueCommands::AddToQueueFromUrl => Self::add_to_queue_from_url(None, None).await,
            QueueCommands::AddToQueueFromUrlList => Self::add_to_queue_from_url_list().await,
            QueueCommands::ResumeQueue => Self::resume_queue().await,
            QueueCommands::SaveToAvailableTracks => Self::add_to_available_downloads().await,
            QueueCommands::DownloadOnlyAvailableTracks => {
                let mut soundeo_user = SoundeoUser::new().change_context(QueueError)?;
                soundeo_user
                    .login_and_update_user_info()
                    .await
                    .change_context(QueueError)?;
                Self::download_available_tracks(&mut soundeo_user).await
            }
        };
    }

    fn get_options() -> Vec<String> {
        Self::iter()
            .map(|element| element.to_string().to_sentence_case())
            .collect::<Vec<_>>()
    }

    fn get_selection(selection: usize) -> Self {
        let options = Self::iter().collect::<Vec<_>>();
        options[selection].clone()
    }

    async fn add_to_queue_from_url_list() -> QueueResult<()> {
        let url_list = DjWizardLog::get_url_list().change_context(QueueError)?;
        let prompt_text = format!("Do you want to download the already downloded tracks again?");
        let repeat_download =
            Dialoguer::select_yes_or_no(prompt_text).change_context(QueueError)?;

        for url in url_list {
            println!("Queueing tracks from: {}", url.cyan());
            match Self::add_to_queue_from_url(Some(url.clone()), Some(repeat_download)).await {
                Ok(_) => {
                    println!("Url successfully queued: {}", url.clone().green());
                    let removed = DjWizardLog::remove_url_from_url_list(url.clone())
                        .change_context(QueueError)?;
                    if !removed {
                        println!("Url could not be removed from the list: {}", url.red());
                    }
                }
                Err(err) => {
                    println!(
                        "Url: {} \n Could not be queued due to: {}",
                        url.clone().yellow(),
                        err
                    );
                }
            };
        }
        Ok(())
    }

    async fn add_to_queue_from_url(
        soundeo_url: Option<String>,
        repeat_download: Option<bool>,
    ) -> QueueResult<()> {
        let soundeo_url_string = match soundeo_url {
            Some(soundeo_url_string) => soundeo_url_string,
            None => {
                let prompt_text = format!("Soundeo url: ");
                let url = Dialoguer::input(prompt_text).change_context(QueueError)?;
                let soundeo_url = Url::parse(&url).into_report().change_context(QueueError)?;
                soundeo_url.to_string()
            }
        };

        let repeat_download_result = match repeat_download {
            Some(repeat_download_bool) => repeat_download_bool,
            None => {
                let prompt_text =
                    format!("Do you want to download the already downloded tracks again?");
                let repeat_download =
                    Dialoguer::select_yes_or_no(prompt_text).change_context(QueueError)?;
                repeat_download
            }
        };

        let mut soundeo_user = SoundeoUser::new().change_context(QueueError)?;
        soundeo_user
            .login_and_update_user_info()
            .await
            .change_context(QueueError)?;
        let mut track_list =
            SoundeoTracksList::new(soundeo_url_string).change_context(QueueError)?;
        track_list
            .get_tracks_id(&soundeo_user)
            .await
            .change_context(QueueError)?;
        println!(
            "Queueing {} tracks",
            format!("{}", track_list.track_ids.len()).cyan()
        );

        let available_tracks = DjWizardLog::get_available_tracks().change_context(QueueError)?;

        for (track_id_index, track_id) in track_list.track_ids.iter().enumerate() {
            println!(
                "-----------------------------------------------------------------------------"
            );

            println!(
                "Queueing track {} of {}",
                track_id_index + 1,
                track_list.track_ids.len()
            );
            let mut track_info = SoundeoTrack::new(track_id.clone());
            track_info
                .get_info(&soundeo_user, true)
                .await
                .change_context(QueueError)?;

            if track_info.already_downloaded {
                if !repeat_download_result {
                    track_info.print_already_downloaded();
                    continue;
                } else {
                    track_info.print_downloading_again();
                    track_info
                        .reset_already_downloaded(&mut soundeo_user)
                        .await
                        .change_context(QueueError)?;
                }
            }

            if let Some(_) = available_tracks.get(track_id) {
                track_info.print_already_available();
                continue;
            }

            let queue_result =
                DjWizardLog::add_queued_track(track_id.clone()).change_context(QueueError)?;
            if queue_result {
                println!(
                    "Track with id {} successfully queued",
                    track_id.clone().green(),
                );
            } else {
                println!(
                    "Track with id {} was previously queued, skipping",
                    track_id.clone().yellow(),
                );
            }
        }
        Ok(())
    }
    async fn add_to_available_downloads() -> QueueResult<()> {
        let prompt_text = format!("Soundeo url: ");
        let url = Dialoguer::input(prompt_text).change_context(QueueError)?;
        let soundeo_url = Url::parse(&url).into_report().change_context(QueueError)?;
        let mut soundeo_user = SoundeoUser::new().change_context(QueueError)?;
        soundeo_user
            .login_and_update_user_info()
            .await
            .change_context(QueueError)?;
        let mut track_list =
            SoundeoTracksList::new(soundeo_url.to_string()).change_context(QueueError)?;
        track_list
            .get_tracks_id(&soundeo_user)
            .await
            .change_context(QueueError)?;
        println!(
            "Saving {} tracks",
            format!("{}", track_list.track_ids.len()).cyan()
        );

        let available_tracks = DjWizardLog::get_available_tracks().change_context(QueueError)?;
        let queued_tracks = DjWizardLog::get_queued_tracks().change_context(QueueError)?;

        let prompt_text = format!("Do you want to download the already downloded tracks again?");
        let repeat_download =
            Dialoguer::select_yes_or_no(prompt_text).change_context(QueueError)?;

        for (track_id_index, track_id) in track_list.track_ids.iter().enumerate() {
            println!(
                "-----------------------------------------------------------------------------"
            );
            println!(
                "Saving track {} of {}",
                track_id_index + 1,
                track_list.track_ids.len()
            );
            let mut track_info = SoundeoTrack::new(track_id.clone());
            track_info
                .get_info(&soundeo_user, true)
                .await
                .change_context(QueueError)?;

            if track_info.already_downloaded {
                if !repeat_download {
                    track_info.print_already_downloaded();
                    continue;
                } else {
                    track_info.print_downloading_again();
                    track_info
                        .reset_already_downloaded(&mut soundeo_user)
                        .await
                        .change_context(QueueError)?;
                }
            }

            if let Some(_) = available_tracks.get(track_id) {
                track_info.print_already_available();
                continue;
            }

            println!(
                "Adding {} to the available tracks queue",
                track_info.title.cyan()
            );
            let download_url_result = track_info.get_download_url(&mut soundeo_user).await;
            match download_url_result {
                Ok(_) => {
                    DjWizardLog::add_available_track(track_id.clone())
                        .change_context(QueueError)?;
                    if queued_tracks.get(&track_id.clone()).is_some() {
                        DjWizardLog::remove_queued_track(track_id.clone())
                            .change_context(QueueError)?;
                    }
                    println!(
                        "Track successfully added to the {} and available for download",
                        "Available tracks queue".green()
                    );
                    if queued_tracks.get(&track_id.clone()).is_some() {
                        DjWizardLog::remove_queued_track(track_id.clone())
                            .change_context(QueueError)?;
                        println!("Track removed from queue");
                    }
                }
                Err(err) => {
                    DjWizardLog::add_queued_track(track_id.clone()).change_context(QueueError)?;
                    println!("Error adding track to the collection:\n{:#?}", err);
                    println!("Adding to the queue");
                    let queue_result = DjWizardLog::add_queued_track(track_id.clone())
                        .change_context(QueueError)?;
                    if queue_result {
                        println!(
                            "Track with id {} successfully queued",
                            track_id.clone().green(),
                        );
                    } else {
                        println!(
                            "Track with id {} was previously queued",
                            track_id.clone().yellow(),
                        );
                    }
                }
            }
        }
        Ok(())
    }

    async fn resume_queue() -> QueueResult<()> {
        let filtered_by_genre =
            Dialoguer::select_yes_or_no("Do you want to filter by genre".to_string())
                .change_context(QueueError)?;
        let queued_tracks = if filtered_by_genre {
            Self::filter_queue()?
        } else {
            DjWizardLog::get_queued_tracks()
                .change_context(QueueError)?
                .into_iter()
                .collect()
        };
        let mut soundeo_user = SoundeoUser::new().change_context(QueueError)?;
        soundeo_user
            .login_and_update_user_info()
            .await
            .change_context(QueueError)?;
        println!(
            "The queue has {} tracks still pending to download, collecting available downloads",
            format!("{}", queued_tracks.len()).cyan()
        );

        let queued_tracks_length = queued_tracks.len();
        for (track_id_index, track_id) in queued_tracks.clone().into_iter().enumerate() {
            let mut track_info = SoundeoTrack::new(track_id.clone());
            track_info
                .get_info(&soundeo_user, true)
                .await
                .change_context(QueueError)?;
            let download_url_result = track_info.get_download_url(&mut soundeo_user).await;
            match download_url_result {
                Ok(_) => {
                    println!(
                        "{}/{}: Track {} added to the available tracks",
                        track_id_index + 1,
                        queued_tracks_length,
                        track_info.title.green()
                    );
                    DjWizardLog::add_available_track(track_id.clone())
                        .change_context(QueueError)?;
                    DjWizardLog::remove_queued_track(track_id).change_context(QueueError)?;
                }
                _ => {
                    println!(
                        "{}/{}: Track {} can't be downloaded now",
                        track_id_index + 1,
                        queued_tracks_length,
                        track_info.title.yellow()
                    );
                }
            }
        }

        let available_downloads = DjWizardLog::get_available_tracks().change_context(QueueError)?;

        if available_downloads.is_empty() {
            let first_id = queued_tracks.get(0).unwrap().clone();
            let mut track_info = SoundeoTrack::new(first_id.clone());
            track_info
                .get_info(&soundeo_user, true)
                .await
                .change_context(QueueError)?;
            return track_info
                .download_track(&mut soundeo_user, true)
                .await
                .change_context(QueueError);
        }

        println!("Downloading from the {} queue", "available tracks".green());

        Self::download_available_tracks(&mut soundeo_user).await?;
        Ok(())
    }

    async fn download_available_tracks(soundeo_user: &mut SoundeoUser) -> QueueResult<()> {
        let available_tracks: HashSet<String> = DjWizardLog::get_available_tracks()
            .change_context(QueueError)?
            .into_iter()
            .collect();

        if available_tracks.is_empty() {
            println!("No available to download tracks",);
            return Ok(());
        }

        println!(
            "{} available to download tracks",
            format!("{}", available_tracks.len()).cyan()
        );

        for (available_id_index, available_id) in available_tracks.clone().into_iter().enumerate() {
            println!(
                "Downloading track {} of {}",
                (available_id_index + 1).to_string().cyan(),
                available_tracks.len().to_string().cyan()
            );
            let mut track_info = SoundeoTrack::new(available_id.clone());
            let download_result = track_info
                .download_track(soundeo_user, false)
                .await
                .change_context(QueueError);
            match download_result {
                Ok(_) => {
                    DjWizardLog::remove_available_track(available_id.clone())
                        .change_context(QueueError)?;
                }
                Err(error) => {
                    println!(
                        "Track with id {} was not downloaded",
                        available_id.clone().red()
                    );
                    println!("Error: {:?}", error)
                }
            }
        }
        Ok(())
    }

    fn filter_queue() -> QueueResult<Vec<String>> {
        let Soundeo { tracks_info } = DjWizardLog::get_soundeo().change_context(QueueError)?;
        // let tracks = soundeo
        let queued_tracks = DjWizardLog::get_queued_tracks().change_context(QueueError)?;
        let q_tracks_info: Vec<SoundeoTrack> = queued_tracks
            .into_iter()
            .map(|q_track| tracks_info.get(&q_track).unwrap().clone())
            .collect();
        let mut genres_hash_set = HashSet::new();
        for track in q_tracks_info.clone() {
            genres_hash_set.insert(track.genre);
        }
        let mut genres = genres_hash_set.into_iter().collect::<Vec<String>>();
        genres.sort();
        let selection = Dialoguer::select("Select the genre".to_string(), genres.clone(), None)
            .change_context(QueueError)?;
        let selected_genre = genres[selection].clone();
        let selected_tracks = q_tracks_info
            .clone()
            .into_iter()
            .filter_map(|track| {
                if track.genre == selected_genre {
                    Some(track.id)
                } else {
                    None
                }
            })
            .collect::<Vec<String>>();
        Ok(selected_tracks)
    }
}
