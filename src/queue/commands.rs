use colored::Colorize;
use error_stack::{IntoReport, ResultExt};
use inflector::Inflector;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use strum::IntoEnumIterator;
use url::Url;

use crate::artist::{ArtistCRUD, ArtistManager};
use crate::dialoguer::Dialoguer;
use crate::log::{DjWizardLog, Priority, QueuedTrack};
use crate::queue::track_processor::TrackProcessor;
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
    CleanDownloadedFromQueue,
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
            QueueCommands::CleanDownloadedFromQueue => {
                Self::clean_downloaded_from_queue().change_context(QueueError)
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
            SoundeoTracksList::new(soundeo_url_string.clone()).change_context(QueueError)?;
        track_list
            .get_tracks_id(&soundeo_user)
            .await
            .change_context(QueueError)?;
        println!(
            "Queueing {} tracks",
            format!("{}", track_list.track_ids.len()).cyan()
        );

        // Use the common track processor with detailed progress
        let context_description = format!("from URL: {}", soundeo_url_string);
        
        TrackProcessor::process_tracks_to_queue(
            &track_list.track_ids,
            &soundeo_user,
            selected_priority,
            repeat_download_result,
            &context_description,
        )
        .await
        .change_context(QueueError)?;
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

            // Check if track is downloadable before trying to get download URL
            if !track_info.downloadable {
                track_info.print_not_downloadable();
                println!("Skipping track {} ({}) as it's not downloadable", 
                    track_info.title.yellow(), 
                    track_info.get_track_url().yellow());
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
            
            // Check if track is downloadable, if not, remove from queue
            if !track_info.downloadable {
                println!(
                    "{}/{}: Track {} ({}) is not downloadable, removing from queue",
                    track_id_index + 1,
                    queued_tracks_length,
                    track_info.title.red(),
                    track_info.get_track_url().yellow()
                );
                track_info.print_not_downloadable();
                DjWizardLog::remove_queued_track(queued_track.track_id.clone())
                    .change_context(QueueError)?;
                continue;
            }
            
            // Check remaining downloads before getting download URL (which consumes downloads)
            let (main_downloads, bonus_downloads) = soundeo_user
                .check_remaining_downloads()
                .await
                .change_context(QueueError)?;
            
            if main_downloads + bonus_downloads == 0 {
                println!(
                    "{}/{}: No downloads remaining. Stopping URL collection and proceeding with available tracks.",
                    track_id_index + 1,
                    queued_tracks_length
                );
                break;
            }
            
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
                        "{}/{}: Track {} ({}) can't be downloaded now",
                        track_id_index + 1,
                        queued_tracks_length,
                        track_info.title.yellow(),
                        track_info.get_track_url().yellow()
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
                        "Track {} ({}) with id {} was not downloaded",
                        track_info.title.red(),
                        track_info.get_track_url().yellow(),
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
            "Prioritize by Artist",
            "Prioritize by Spotify Playlist",
            "Prioritize by Genre",
        ];
        let selection = Dialoguer::select(
            "How do you want to manage the queue?".to_string(),
            options,
            None,
        )
        .change_context(QueueError)?;

        match selection {
            0 => Self::prioritize_by_artist().await?,
            1 => Self::prioritize_by_spotify_playlist().await?,
            2 => Self::prioritize_by_genre().await?,
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

        // Get saved artists
        let artist_manager = DjWizardLog::get_artist_manager().change_context(QueueError)?;
        let saved_artists = artist_manager.get_all_artists();

        let artist_queries: Vec<String> = if saved_artists.is_empty() {
            // No saved artists, ask for new one
            println!("{}", "No saved artists found.".yellow());
            let artist_name = Dialoguer::input("Enter artist name to search for:".to_string())
                .change_context(QueueError)?;
            if artist_name.trim().is_empty() {
                println!("Artist name cannot be empty.");
                return Ok(());
            }
            
            // Add to saved artists
            let formatted_name = ArtistManager::format_artist_name(&artist_name);
            let mut manager = artist_manager;
            manager.add_artist(&formatted_name, None).change_context(QueueError)?;
            DjWizardLog::save_artist_manager(manager).change_context(QueueError)?;
            println!("Artist '{}' added to favorites!", formatted_name.green());
            
            vec![formatted_name]
        } else {
            // Ask user if they want to use saved artists or enter new one
            let options = vec![
                "Use saved favorite artists",
                "Enter new artist name",
            ];
            let selection = Dialoguer::select(
                "How do you want to prioritize by artist?".to_string(),
                options,
                Some(0),
            )
            .change_context(QueueError)?;

            match selection {
                0 => {
                    // Use saved artists
                    let artist_names: Vec<String> = saved_artists
                        .iter()
                        .map(|artist| artist.name.clone())
                        .collect();

                    // Ask if they want all artists or select specific ones
                    let use_all = Dialoguer::confirm(
                        format!("Do you want to prioritize from all {} saved artists?", artist_names.len()),
                        Some(true),
                    )
                    .change_context(QueueError)?;

                    if use_all {
                        artist_names
                    } else {
                        // Show multiselect with all artists selected by default
                        let selections = Dialoguer::multiselect(
                            "Select artists to prioritize (all selected by default, spacebar to toggle, enter to confirm):".to_string(),
                            artist_names.clone(),
                            Some(&vec![true; artist_names.len()]),
                            false,
                        )
                        .change_context(QueueError)?;

                        if selections.is_empty() {
                            println!("No artists selected.");
                            return Ok(());
                        }

                        selections
                            .iter()
                            .map(|&index| artist_names[index].clone())
                            .collect()
                    }
                }
                _ => {
                    // Enter new artist
                    let artist_name = Dialoguer::input("Enter artist name to search for:".to_string())
                        .change_context(QueueError)?;
                    if artist_name.trim().is_empty() {
                        println!("Artist name cannot be empty.");
                        return Ok(());
                    }
                    
                    // Add to saved artists
                    let formatted_name = ArtistManager::format_artist_name(&artist_name);
                    let mut manager = artist_manager;
                    let added = manager.add_artist(&formatted_name, None).change_context(QueueError)?;
                    DjWizardLog::save_artist_manager(manager).change_context(QueueError)?;
                    
                    if added {
                        println!("Artist '{}' added to favorites!", formatted_name.green());
                    } else {
                        println!("Artist '{}' already in favorites.", formatted_name.yellow());
                    }
                    
                    vec![formatted_name]
                }
            }
        };

        // Find matching tracks for all selected artists
        let mut all_matching_tracks: Vec<&QueuedTrack> = Vec::new();
        
        for artist_query in &artist_queries {
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
            
            all_matching_tracks.extend(matching_tracks);
        }

        // Remove duplicates
        all_matching_tracks.sort_by_key(|track| &track.track_id);
        all_matching_tracks.dedup_by_key(|track| &track.track_id);

        if all_matching_tracks.is_empty() {
            if artist_queries.len() == 1 {
                println!("No tracks found in the queue matching '{}'.", artist_queries[0]);
            } else {
                println!("No tracks found in the queue matching any of the selected artists.");
            }
            return Ok(());
        }

        println!(
            "Found {} matching tracks for {} artist(s)",
            all_matching_tracks.len().to_string().cyan(),
            artist_queries.len().to_string().cyan()
        );

        let track_titles: Vec<String> = all_matching_tracks
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
            Some(&vec![true; all_matching_tracks.len()]),
            false,
        )
        .change_context(QueueError)?;

        if !selections.is_empty() {
            let track_ids_to_promote: Vec<String> = selections
                .iter()
                .map(|&index| all_matching_tracks[index].track_id.clone())
                .collect();
            
            println!(
                "Prioritizing {} tracks to the top of the queue...",
                track_ids_to_promote.len().to_string().green()
            );
            
            DjWizardLog::promote_tracks_to_top(&track_ids_to_promote).change_context(QueueError)?;
            
            println!("Successfully prioritized tracks!");
        } else {
            println!("No tracks selected for prioritization.");
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
                .pair_unpaired_tracks(&mut soundeo_user)
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

    fn clean_downloaded_from_queue() -> QueueResult<()> {
        println!(
            "\n{}",
            "Cleaning already downloaded tracks from the queue...".yellow()
        );

        // 1. Get all necessary data from the log
        let soundeo_log = DjWizardLog::get_soundeo().change_context(QueueError)?;
        let queued_tracks = DjWizardLog::get_queued_tracks().change_context(QueueError)?;

        if queued_tracks.is_empty() {
            println!(
                "{}",
                "The queue is already empty. Nothing to clean.".green()
            );
            return Ok(());
        }

        let mut removed_count = 0;

        // 2. Iterate and remove
        for queued_track in queued_tracks {
            if let Some(track_info) = soundeo_log.tracks_info.get(&queued_track.track_id) {
                if track_info.already_downloaded {
                    if DjWizardLog::remove_queued_track(queued_track.track_id.clone())
                        .change_context(QueueError)?
                    {
                        println!("  - Removing '{}'", track_info.title.cyan());
                        removed_count += 1;
                    }
                }
            }
        }

        if removed_count > 0 {
            println!(
                "\nSuccessfully removed {} downloaded tracks from the queue.",
                removed_count.to_string().green()
            );
        } else {
            println!("\nNo downloaded tracks were found in the queue. Nothing to remove.");
        }

        Ok(())
    }
}
