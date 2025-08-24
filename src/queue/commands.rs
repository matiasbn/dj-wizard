use colored::Colorize;
use error_stack::{IntoReport, ResultExt};
use inflector::Inflector;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use strum::IntoEnumIterator;
use url::Url;

use crate::dialoguer::Dialoguer;
use crate::log::{DjWizardLog, Priority, QueuedTrack};
use crate::queue::{QueueError, QueueResult};
use crate::soundeo::track::SoundeoTrack;
use crate::soundeo::track_list::SoundeoTracksList;
use crate::soundeo::Soundeo;
use crate::spotify::playlist::SpotifyPlaylist;
use crate::url_list::UrlListCRUD;
use crate::user::SoundeoUser;

#[derive(Debug, Deserialize, Serialize, Clone, strum_macros::Display, strum_macros::EnumIter)]
pub enum QueueCommands {
    ResumeQueue,
    ManageQueue,
    GetQueueInfo,
    SaveToAvailableTracks,
    AddToQueueFromUrl,
    AddToQueueFromUrlList,
    DownloadOnlyAvailableTracks,
}

impl QueueCommands {
    pub async fn execute(resume_queue_flag: bool) -> QueueResult<()> {
        if resume_queue_flag {
            return Self::resume_queue(resume_queue_flag).await;
        }
        let options = Self::get_options();
        let selection = Dialoguer::select("What you want to do?".to_string(), options, None)
            .change_context(QueueError)?;
        return match Self::get_selection(selection) {
            QueueCommands::AddToQueueFromUrl => Self::add_to_queue_from_url(None, None).await,
            QueueCommands::AddToQueueFromUrlList => Self::add_to_queue_from_url_list().await,
            QueueCommands::ResumeQueue => Self::resume_queue(resume_queue_flag).await,
            QueueCommands::SaveToAvailableTracks => Self::add_to_available_downloads().await,
            QueueCommands::GetQueueInfo => Self::get_queue_information(),
            QueueCommands::ManageQueue => Self::manage_queue().await,
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

    fn prompt_for_priority() -> QueueResult<Priority> {
        let priority_options = vec!["High (download first)", "Normal", "Low (download last)"];
        let selection = Dialoguer::select(
            "Choose a priority for this batch of songs".to_string(),
            priority_options,
            Some(1), // "Normal" as option by default
        )
        .change_context(QueueError)?;

        let selected_priority = match selection {
            0 => Priority::High,
            1 => Priority::Normal,
            _ => Priority::Low,
        };
        Ok(selected_priority)
    }

    async fn add_to_queue_from_url_list() -> QueueResult<()> {
        let url_list = DjWizardLog::get_url_list().change_context(QueueError)?;
        let prompt_text = format!("Do you want to download the already downloaded tracks again?");
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

        let selected_priority = Self::prompt_for_priority()?;

        let repeat_download_result = match repeat_download {
            Some(repeat_download_bool) => repeat_download_bool,
            None => {
                let prompt_text =
                    format!("Do you want to download the already downloaded tracks again?");
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

            let queue_result = DjWizardLog::add_queued_track(track_id.clone(), selected_priority)
                .change_context(QueueError)?;
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

        let prompt_text = format!("Do you want to download the already downloaded tracks again?");
        let repeat_download =
            Dialoguer::select_yes_or_no(prompt_text).change_context(QueueError)?;

        println!(
            "\n{}",
            "If a track cannot be added to the collection, it will be added to the queue with High priority.".yellow()
        );
        let fallback_priority = Priority::High;

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
                    if queued_tracks.iter().any(|t| t.track_id == *track_id) {
                        DjWizardLog::remove_queued_track(track_id.clone())
                            .change_context(QueueError)?;
                        println!("Track removed from queue");
                    }
                    println!(
                        "Track successfully added to the {} and available for download",
                        "Available tracks queue".green()
                    );
                }
                Err(err) => {
                    println!("Error adding track to the collection:\n{:#?}", err);
                    println!("Adding to the queue with priority {:?}", fallback_priority);
                    let queue_result =
                        DjWizardLog::add_queued_track(track_id.clone(), fallback_priority)
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

    pub async fn resume_queue(resume_queue_flag: bool) -> QueueResult<()> {
        // If the resume queue flag is provided, skip the dialog to filter by genre
        // since we need complete automation
        let filtered_by_genre = if resume_queue_flag {
            false
        } else {
            Dialoguer::select_yes_or_no("Do you want to filter by genre".to_string())
                .change_context(QueueError)?
        };
        let mut queued_tracks = if filtered_by_genre {
            Self::filter_queue()?
        } else {
            DjWizardLog::get_queued_tracks().change_context(QueueError)?
        };

        // Sort the queue by priority and then by order_key
        queued_tracks.sort_by(|a, b| {
            let priority_ord = a.priority.cmp(&b.priority);
            if priority_ord != std::cmp::Ordering::Equal {
                return priority_ord;
            }
            a.order_key.partial_cmp(&b.order_key).unwrap()
        });

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
        for (track_id_index, queued_track) in queued_tracks.iter().enumerate() {
            let mut track_info = SoundeoTrack::new(queued_track.track_id.clone());
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
                    DjWizardLog::add_available_track(queued_track.track_id.clone())
                        .change_context(QueueError)?;
                    DjWizardLog::remove_queued_track(queued_track.track_id.clone())
                        .change_context(QueueError)?;
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
            if queued_tracks.is_empty() {
                println!("{}", "The queue is empty. Nothing to do.".yellow());
                return Ok(());
            }
            let first_id = &queued_tracks.get(0).unwrap().track_id;
            let mut track_info = SoundeoTrack::new(first_id.to_string());
            track_info
                .get_info(&soundeo_user, true)
                .await
                .change_context(QueueError)?;
            return track_info
                .download_track(&mut soundeo_user, true, false)
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
            println!("No tracks available to download");
            return Ok(());
        }

        println!(
            "{} tracks available to download",
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
                .download_track(soundeo_user, false, false)
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

    fn get_queue_information() -> QueueResult<()> {
        let Soundeo { tracks_info, .. } = DjWizardLog::get_soundeo().change_context(QueueError)?;
        let queued_tracks = DjWizardLog::get_queued_tracks().change_context(QueueError)?;
        let q_tracks_info: Vec<SoundeoTrack> = queued_tracks
            .iter()
            .filter_map(|q_track| tracks_info.get(&q_track.track_id))
            .cloned()
            .collect();
        let mut genres_hash_set = HashSet::new();
        for track in q_tracks_info.clone() {
            genres_hash_set.insert(track.genre);
        }
        let mut genres = genres_hash_set.into_iter().collect::<Vec<String>>();
        genres.sort();
        for genre in genres {
            let amount = q_tracks_info
                .clone()
                .into_iter()
                .filter(|track| track.genre == genre)
                .count();
            println!("{}: {} tracks", genre.cyan(), amount);
        }
        println!(
            "{}: {} tracks",
            format!("Total").green(),
            q_tracks_info.len()
        );

        Ok(())
    }

    fn filter_queue() -> QueueResult<Vec<QueuedTrack>> {
        let Soundeo { tracks_info, .. } = DjWizardLog::get_soundeo().change_context(QueueError)?;
        let queued_tracks = DjWizardLog::get_queued_tracks().change_context(QueueError)?;

        let mut genres_hash_set = HashSet::new();
        for q_track in &queued_tracks {
            if let Some(track_info) = tracks_info.get(&q_track.track_id) {
                genres_hash_set.insert(track_info.genre.clone());
            }
        }

        let mut genres = genres_hash_set.into_iter().collect::<Vec<String>>();
        genres.sort();
        let selection = Dialoguer::select("Select the genre".to_string(), genres.clone(), None)
            .change_context(QueueError)?;
        let selected_genre = &genres[selection];

        let selected_tracks = queued_tracks
            .into_iter()
            .filter(|q_track| {
                tracks_info
                    .get(&q_track.track_id)
                    .map_or(false, |info| &info.genre == selected_genre)
            })
            .collect();
        Ok(selected_tracks)
    }

    async fn manage_queue() -> QueueResult<()> {
        let options = vec![
            "Prioritize by Spotify Playlist",
            "Prioritize by Genre",
            "Prioritize by Artist",
        ];
        let selection = Dialoguer::select(
            "How do you want to manage the queue?".to_string(),
            options,
            None,
        )
        .change_context(QueueError)?;

        match selection {
            0 => Self::prioritize_by_spotify_playlist().await?,
            1 => Self::prioritize_by_genre().await?,
            2 => Self::prioritize_by_artist().await?,
            _ => unreachable!(),
        }
        Ok(())
    }

    async fn prioritize_by_genre() -> QueueResult<()> {
        let soundeo_info = DjWizardLog::get_soundeo().change_context(QueueError)?;
        let queued_tracks = DjWizardLog::get_queued_tracks().change_context(QueueError)?;

        if queued_tracks.is_empty() {
            println!("The queue is empty. Nothing to manage.");
            return Ok(());
        }

        let mut genres_in_queue = HashSet::new();
        for track in &queued_tracks {
            if let Some(track_info) = soundeo_info.tracks_info.get(&track.track_id) {
                genres_in_queue.insert(track_info.genre.clone());
            }
        }

        if genres_in_queue.is_empty() {
            println!("No genre information available for the tracks in the queue.");
            return Ok(());
        }

        let mut genres: Vec<String> = genres_in_queue.into_iter().collect();
        genres.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()));

        let selection = Dialoguer::select(
            "Select a genre to prioritize".to_string(),
            genres.clone(),
            None,
        )
        .change_context(QueueError)?;

        let selected_genre = &genres[selection];

        let track_ids_to_promote: Vec<String> = queued_tracks
            .iter()
            .filter(|q_track| {
                soundeo_info
                    .tracks_info
                    .get(&q_track.track_id)
                    .map_or(false, |info| &info.genre == selected_genre)
            })
            .map(|q_track| q_track.track_id.clone())
            .collect();

        if track_ids_to_promote.is_empty() {
            println!("No tracks found for the selected genre.");
        } else {
            DjWizardLog::promote_tracks_to_top(&track_ids_to_promote).change_context(QueueError)?;
        }

        Ok(())
    }

    async fn prioritize_by_artist() -> QueueResult<()> {
        let soundeo_info = DjWizardLog::get_soundeo().change_context(QueueError)?;
        let queued_tracks = DjWizardLog::get_queued_tracks().change_context(QueueError)?;

        if queued_tracks.is_empty() {
            println!("The queue is empty. Nothing to manage.");
            return Ok(());
        }

        let artist_query = Dialoguer::input("Enter artist name to search for:".to_string())
            .change_context(QueueError)?;
        if artist_query.trim().is_empty() {
            println!("Search query cannot be empty.");
            return Ok(());
        }

        let matching_tracks: Vec<&QueuedTrack> = queued_tracks
            .iter()
            .filter(|q_track| {
                soundeo_info
                    .tracks_info
                    .get(&q_track.track_id)
                    .map_or(false, |info| {
                        info.title
                            .to_lowercase()
                            .contains(&artist_query.to_lowercase())
                    })
            })
            .collect();

        if matching_tracks.is_empty() {
            println!("No tracks found in the queue matching your search.");
            return Ok(());
        }

        let track_titles: Vec<String> = matching_tracks
            .iter()
            .map(|q_track| {
                soundeo_info
                    .tracks_info
                    .get(&q_track.track_id)
                    .map_or_else(|| "Unknown Track".to_string(), |info| info.title.clone())
            })
            .collect();

        let selections = Dialoguer::multiselect(
            "All matching tracks are selected. Deselect any you want to DISCARD (spacebar to toggle, enter to confirm):"
                .to_string(),
            track_titles,
            Some(&vec![true; matching_tracks.len()]),
            false,
        )
        .change_context(QueueError)?;

        if !selections.is_empty() {
            let track_ids_to_promote: Vec<String> = selections
                .iter()
                .map(|&index| matching_tracks[index].track_id.clone())
                .collect();
            DjWizardLog::promote_tracks_to_top(&track_ids_to_promote).change_context(QueueError)?;
        }

        Ok(())
    }

    async fn prioritize_by_spotify_playlist() -> QueueResult<()> {
        let mut playlist =
            SpotifyPlaylist::prompt_select_playlist("Select a Spotify playlist to prioritize")
                .change_context(QueueError)?;

        let spotify_log = DjWizardLog::get_spotify().change_context(QueueError)?;

        // 1. Check for unpaired tracks
        let unpaired_track_ids: Vec<String> = playlist
            .tracks
            .keys()
            .filter(|spotify_id| !spotify_log.soundeo_track_ids.contains_key(*spotify_id))
            .cloned()
            .collect();

        let mut pair_tracks = true;
        if !unpaired_track_ids.is_empty() {
            println!(
                "The playlist '{}' has {} unpaired tracks.",
                playlist.name.yellow(),
                unpaired_track_ids.len().to_string().yellow()
            );
            let options = vec![
                "Yes, pair them now and prioritize all tracks",
                "No, prioritize only the already-paired tracks",
            ];
            let selection = Dialoguer::select(
                "Do you want to find the Soundeo equivalent for these tracks first?".to_string(),
                options,
                Some(0),
            )
            .change_context(QueueError)?;

            if selection == 1 {
                pair_tracks = false;
            }
        }

        if pair_tracks && !unpaired_track_ids.is_empty() {
            println!("Logging into Soundeo to pair tracks...");
            let mut soundeo_user = SoundeoUser::new().change_context(QueueError)?;
            soundeo_user
                .login_and_update_user_info()
                .await
                .change_context(QueueError)?;

            playlist
                .pair_unpaired_tracks(&mut soundeo_user, Priority::High)
                .await
                .change_context(QueueError)?;
        }

        // Refetch the log to get the latest pairings
        let final_spotify_log = DjWizardLog::get_spotify().change_context(QueueError)?;
        let queued_tracks = DjWizardLog::get_queued_tracks().change_context(QueueError)?;
        let queued_track_ids: HashSet<String> =
            queued_tracks.iter().map(|t| t.track_id.clone()).collect();

        let mut track_ids_to_promote: Vec<String> = Vec::new();
        for spotify_track_id in playlist.tracks.keys() {
            if let Some(Some(soundeo_track_id)) =
                final_spotify_log.soundeo_track_ids.get(spotify_track_id)
            {
                if queued_track_ids.contains(soundeo_track_id) {
                    track_ids_to_promote.push(soundeo_track_id.clone());
                }
            }
        }

        if track_ids_to_promote.is_empty() {
            println!(
                "{}",
                format!(
                    "No tracks from the playlist '{}' are currently in the queue.",
                    playlist.name
                )
                .yellow()
            );
        } else {
            DjWizardLog::promote_tracks_to_top(&track_ids_to_promote).change_context(QueueError)?;
        }

        Ok(())
    }
}
