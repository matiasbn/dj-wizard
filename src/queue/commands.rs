use clap::builder::Str;
use colored::Colorize;
use error_stack::{IntoReport, Report, ResultExt};
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
use crate::user::SoundeoUser;

#[derive(Debug, Deserialize, Serialize, Clone, strum_macros::Display, strum_macros::EnumIter)]
pub enum QueueCommands {
    AddToQueue,
    ResumeQueue,
}

impl QueueCommands {
    pub async fn execute() -> QueueResult<()> {
        let options = Self::get_options();
        let selection = Dialoguer::select("What you want to do?".to_string(), options, None)
            .change_context(QueueError)?;
        return match Self::get_selection(selection) {
            QueueCommands::AddToQueue => Self::add_to_queue().await,
            QueueCommands::ResumeQueue => Self::resume_queue().await,
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

    async fn add_to_queue() -> QueueResult<()> {
        let prompt_text = format!("Soundeo url: ");
        let url = Dialoguer::input(prompt_text).change_context(QueueError)?;
        let add_to_collection = Dialoguer::select_yes_or_no(format!(
            "Do you want to add the tracks to your {} collection?",
            "Soundeo".cyan()
        ))
        .change_context(QueueError)?;
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
            "Queueing {} tracks",
            format!("{}", track_list.track_ids.len()).cyan()
        );
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
                track_info.print_already_downloaded();
                continue;
            }

            let queue_result =
                DjWizardLog::enqueue_track_to_log(track_id.clone()).change_context(QueueError)?;
            if queue_result {
                println!(
                    "Track with id {} successfully queued",
                    track_id.clone().green(),
                );
                if add_to_collection {
                    println!(
                        "Adding {} to the Soundeo collection",
                        track_info.title.cyan()
                    );
                    let download_url_result = track_info.get_download_url(&mut soundeo_user).await;
                    match download_url_result {
                        Ok(url) => {
                            println!(
                                "Track successfully added to the {}",
                                "Soundeo collection".green()
                            );
                        }
                        Err(err) => {
                            println!("Error adding track to the collection:\n{:#?}", err);
                        }
                    }
                }
            } else {
                println!(
                    "Track with id {} was previously queued",
                    track_id.clone().yellow(),
                );
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
            "The queue has {} tracks still pending to download",
            format!("{}", queued_tracks.len()).cyan()
        );
        for track_id in queued_tracks {
            let mut track_info = SoundeoTrack::new(track_id.clone());
            let download_result = track_info
                .download_track(&mut soundeo_user)
                .await
                .change_context(QueueError);
            match download_result {
                Ok(_) => {
                    DjWizardLog::remove_queued_track_from_log(track_id.clone())
                        .change_context(QueueError)?;
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
        Ok(())
    }

    fn filter_queue() -> QueueResult<Vec<String>> {
        let Soundeo { tracks_info } = DjWizardLog::get_soundeo().change_context(QueueError)?;
        // let tracks = soundeo
        let queued_tracks = DjWizardLog::get_queued_tracks().change_context(QueueError)?;
        let mut q_tracks_info: Vec<SoundeoTrack> = queued_tracks
            .into_iter()
            .map(|q_track| tracks_info.get(&q_track).unwrap().clone())
            .collect();
        let mut genres_hash_set = HashSet::new();
        for track in q_tracks_info.clone() {
            genres_hash_set.insert(track.genre);
        }
        let genres = genres_hash_set.into_iter().collect::<Vec<String>>();
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
