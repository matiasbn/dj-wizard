use std::collections::HashSet;

use chrono::{Duration, Utc};
use colored::Colorize;
use error_stack::{IntoReport, ResultExt};
use inflector::Inflector;
use reqwest::Client;
use strum::IntoEnumIterator;

use crate::dialoguer::Dialoguer;
use crate::genre_tracker::{GenreTrackerCRUD, GenreTrackerError, GenreTrackerResult};
use crate::log::{DjWizardLog, Priority};
use crate::queue::track_processor::TrackProcessor;
use crate::soundeo::track::SoundeoTrack;
use crate::soundeo::track_list::SoundeoTracksList;
use crate::user::SoundeoUser;

#[derive(Debug, Clone, strum_macros::Display, strum_macros::EnumIter)]
pub enum GenreTrackerCommands {
    UpdateFollowedGenre,
    FollowNewGenre,
    ViewFollowedGenres,
    RemoveFollowedGenre,
}

impl GenreTrackerCommands {
    fn select_start_date(genre_name: &str) -> GenreTrackerResult<String> {
        let today = Utc::now();
        let current_year = today
            .format("%Y")
            .to_string()
            .parse::<i32>()
            .unwrap_or(2024);

        // Generate years from 2020 to current year, plus manual option
        let mut years: Vec<String> = (2020..=current_year)
            .rev() // Most recent first
            .map(|y| y.to_string())
            .collect();

        // Add manual input option
        years.push("Custom year (manual input)".to_string());

        let year_selection = Dialoguer::select(
            format!("Select starting year for {} tracking", genre_name),
            years.clone(),
            Some(0), // Default to current year
        )
        .change_context(GenreTrackerError)?;

        let selected_year: i32 = if year_selection == years.len() - 1 {
            // User selected custom year option - loop until valid input
            loop {
                let year_input =
                    Dialoguer::input("Enter custom year (e.g., 2018, 2015): ".to_string())
                        .change_context(GenreTrackerError)?;

                match year_input.parse::<i32>() {
                    Ok(year) if year >= 2010 && year <= current_year + 1 => {
                        println!("Using custom year: {}", year.to_string().cyan());
                        break year;
                    }
                    Ok(year) if year >= 1900 && year < 2010 => {
                        println!(
                            "{}",
                            format!("Year {} is quite old. Are you sure? (y/N)", year).yellow()
                        );
                        let confirmation = Dialoguer::input("Confirm (y/N): ".to_string())
                            .change_context(GenreTrackerError)?;

                        if confirmation.to_lowercase() == "y"
                            || confirmation.to_lowercase() == "yes"
                        {
                            println!("Using custom year: {}", year.to_string().cyan());
                            break year;
                        } else {
                            println!("{}", "Please enter a different year.".yellow());
                            continue;
                        }
                    }
                    Ok(year) if year > current_year + 1 => {
                        println!(
                            "{}",
                            format!("Year {} is in the future. Please enter a valid year.", year)
                                .red()
                        );
                        continue;
                    }
                    Ok(year) => {
                        println!("{}", format!("Year {} seems invalid (too old). Please enter a year from 1900 onwards.", year).red());
                        continue;
                    }
                    Err(_) => {
                        println!(
                            "{}",
                            "Invalid year format. Please enter a valid year (e.g., 2018).".red()
                        );
                        continue;
                    }
                }
            }
        } else {
            years[year_selection].parse().unwrap()
        };

        // Generate months
        let months = vec![
            "January",
            "February",
            "March",
            "April",
            "May",
            "June",
            "July",
            "August",
            "September",
            "October",
            "November",
            "December",
        ];

        let mut default_month = None;

        // If it's current year, default to current month
        if selected_year == current_year {
            let current_month_num = today.format("%m").to_string().parse::<usize>().unwrap_or(1);
            default_month = Some(current_month_num.saturating_sub(1));
        }

        let month_selection = Dialoguer::select(
            format!("Select starting month for {}", selected_year),
            months.iter().map(|m| m.to_string()).collect(),
            default_month,
        )
        .change_context(GenreTrackerError)?;

        // Construct date as first day of selected month
        let start_date = format!("{}-{:02}-01", selected_year, month_selection + 1);

        println!(
            "Will search for {} tracks starting from {} {} ({})",
            genre_name.cyan(),
            months[month_selection].cyan(),
            selected_year.to_string().cyan(),
            start_date.yellow()
        );

        Ok(start_date)
    }

    pub async fn execute() -> GenreTrackerResult<()> {
        let options = Self::get_options();
        let selection = Dialoguer::select(
            "Genre Tracker - What would you like to do?".to_string(),
            options,
            None,
        )
        .change_context(GenreTrackerError)?;

        match Self::get_selection(selection) {
            GenreTrackerCommands::FollowNewGenre => Self::follow_new_genre().await,
            GenreTrackerCommands::UpdateFollowedGenre => Self::update_followed_genre().await,
            GenreTrackerCommands::ViewFollowedGenres => Self::view_followed_genres(),
            GenreTrackerCommands::RemoveFollowedGenre => Self::remove_followed_genre(),
        }
    }

    fn get_options() -> Vec<String> {
        Self::iter()
            .map(|element| element.to_string().to_sentence_case())
            .collect()
    }

    fn get_selection(selection: usize) -> Self {
        let options = Self::iter().collect::<Vec<_>>();
        options[selection].clone()
    }

    async fn follow_new_genre() -> GenreTrackerResult<()> {
        let mut tracker = DjWizardLog::get_genre_tracker().change_context(GenreTrackerError)?;

        // Filter out already tracked genres
        let available_genres: Vec<(u32, String)> = tracker
            .available_genres
            .iter()
            .filter(|(id, _)| !tracker.tracked_genres.contains_key(id))
            .map(|(id, info)| (*id, info.name.clone()))
            .collect();

        if available_genres.is_empty() {
            println!(
                "{}",
                "All available genres are already being tracked!".yellow()
            );
            return Ok(());
        }

        let mut genre_options: Vec<String> = available_genres
            .iter()
            .map(|(_, name)| name.clone())
            .collect();
        genre_options.sort();

        let selection = Dialoguer::select(
            "Select a genre to follow".to_string(),
            genre_options.clone(),
            None,
        )
        .change_context(GenreTrackerError)?;

        let selected_genre_name = &genre_options[selection];
        let genre_id = available_genres
            .iter()
            .find(|(_, name)| name == selected_genre_name)
            .map(|(id, _)| *id)
            .ok_or(GenreTrackerError)
            .into_report()?;

        // Ask for date range using friendly selector
        let start_date = Self::select_start_date(selected_genre_name)?;

        let end_date = Utc::now().format("%Y-%m-%d").to_string();

        println!(
            "Following {} from {} to {}",
            selected_genre_name.cyan(),
            start_date.cyan(),
            end_date.cyan()
        );

        // Add to tracker
        tracker
            .add_tracked_genre(genre_id)
            .change_context(GenreTrackerError)?;

        // Update the last_checked_date to the start_date so we begin from there
        if let Some(tracked_genre) = tracker.tracked_genres.get_mut(&genre_id) {
            tracked_genre.last_checked_date = start_date.clone();
        }

        DjWizardLog::save_genre_tracker(tracker).change_context(GenreTrackerError)?;

        // Now fetch and queue tracks
        Self::fetch_and_queue_tracks(genre_id, &start_date, &end_date).await?;

        Ok(())
    }

    async fn update_followed_genre() -> GenreTrackerResult<()> {
        let mut tracker = DjWizardLog::get_genre_tracker().change_context(GenreTrackerError)?;

        if tracker.tracked_genres.is_empty() {
            println!("{}", "No genres are currently being tracked!".yellow());
            return Ok(());
        }

        let today = Utc::now();
        let mut genre_options: Vec<(u32, String)> = tracker
            .tracked_genres
            .iter()
            .map(|(id, info)| {
                // Calculate days since last check
                let last_date =
                    chrono::NaiveDate::parse_from_str(&info.last_checked_date, "%Y-%m-%d")
                        .unwrap_or_else(|_| today.date_naive());
                let days_ago = (today.date_naive() - last_date).num_days();

                let time_desc = match days_ago {
                    0 => "today".to_string(),
                    1 => "yesterday".to_string(),
                    2..=7 => format!("{} days ago", days_ago),
                    8..=30 => format!("{} days ago", days_ago),
                    31..=365 => format!("{} months ago", days_ago / 30),
                    _ => format!("over a year ago"),
                };

                (
                    *id,
                    format!(
                        "{} (last checked: {} - {})",
                        info.genre_name,
                        info.last_checked_date,
                        time_desc.cyan()
                    ),
                )
            })
            .collect();
        genre_options.sort_by(|a, b| a.1.cmp(&b.1));

        let options: Vec<String> = genre_options.iter().map(|(_, name)| name.clone()).collect();

        let selection = Dialoguer::select("Select a genre to update".to_string(), options, None)
            .change_context(GenreTrackerError)?;

        let genre_id = genre_options[selection].0;
        let tracked_genre = tracker.tracked_genres.get(&genre_id).unwrap();

        // Calculate date range: from last_checked_date (inclusive) to today
        let start_date = tracked_genre.last_checked_date.clone();
        let end_date = Utc::now().format("%Y-%m-%d").to_string();

        println!(
            "Updating {} from {} to {}",
            tracked_genre.genre_name.cyan(),
            start_date.cyan(),
            end_date.cyan()
        );

        // Fetch and queue tracks
        Self::fetch_and_queue_tracks(genre_id, &start_date, &end_date).await?;

        // Update last checked date
        tracker
            .update_last_checked(genre_id)
            .change_context(GenreTrackerError)?;
        DjWizardLog::save_genre_tracker(tracker).change_context(GenreTrackerError)?;

        Ok(())
    }

    fn view_followed_genres() -> GenreTrackerResult<()> {
        let tracker = DjWizardLog::get_genre_tracker().change_context(GenreTrackerError)?;

        if tracker.tracked_genres.is_empty() {
            println!("{}", "No genres are currently being tracked!".yellow());
            return Ok(());
        }

        println!("\n{}", "Currently tracked genres:".green());
        println!("{}", "-".repeat(60));

        let mut genres: Vec<_> = tracker.tracked_genres.values().collect();
        genres.sort_by(|a, b| a.genre_name.cmp(&b.genre_name));

        for genre in genres {
            println!(
                "{}: {} | Created: {} | Last checked: {}",
                genre.genre_name.cyan(),
                format!("ID {}", genre.genre_id).yellow(),
                genre.created_at,
                genre.last_checked_date
            );
        }

        Ok(())
    }

    fn remove_followed_genre() -> GenreTrackerResult<()> {
        let mut tracker = DjWizardLog::get_genre_tracker().change_context(GenreTrackerError)?;

        if tracker.tracked_genres.is_empty() {
            println!("{}", "No genres are currently being tracked!".yellow());
            return Ok(());
        }

        let mut genre_options: Vec<(u32, String)> = tracker
            .tracked_genres
            .iter()
            .map(|(id, info)| (*id, info.genre_name.clone()))
            .collect();
        genre_options.sort_by(|a, b| a.1.cmp(&b.1));

        let options: Vec<String> = genre_options.iter().map(|(_, name)| name.clone()).collect();

        let selection =
            Dialoguer::select("Select a genre to stop tracking".to_string(), options, None)
                .change_context(GenreTrackerError)?;

        let genre_id = genre_options[selection].0;
        let genre_name = genre_options[selection].1.clone();

        tracker.tracked_genres.remove(&genre_id);
        DjWizardLog::save_genre_tracker(tracker).change_context(GenreTrackerError)?;

        println!("Stopped tracking {}", genre_name.green());

        Ok(())
    }

    async fn fetch_and_queue_tracks(
        genre_id: u32,
        start_date: &str,
        end_date: &str,
    ) -> GenreTrackerResult<()> {
        let mut tracker = DjWizardLog::get_genre_tracker().change_context(GenreTrackerError)?;
        
        let mut soundeo_user = SoundeoUser::new().change_context(GenreTrackerError)?;
        soundeo_user
            .login_and_update_user_info()
            .await
            .change_context(GenreTrackerError)?;

        // Phase 1: Find the last page
        println!("Finding the last page...");
        let mut page = 1;
        let mut last_page = None;
        
        loop {
            let url = tracker.build_soundeo_url(genre_id, start_date, end_date, page);
            
            let client = Client::new();
            let session_cookie = soundeo_user
                .get_session_cookie()
                .change_context(GenreTrackerError)?;
            
            let response = client
                .get(&url)
                .header("cookie", &session_cookie)
                .send()
                .await
                .into_report()
                .change_context(GenreTrackerError)?;
            
            if response.status() == 404 {
                last_page = Some(page - 1);
                println!("Found last page: {}", page - 1);
                break;
            }
            
            page += 1;
        }
        
        let last_page = match last_page {
            Some(p) if p > 0 => p,
            _ => {
                println!("No pages found for this date range");
                return Ok(());
            }
        };

        // Phase 2: Process pages from last to first
        println!("Processing {} pages from {} to 1...", last_page, last_page);
        
        let mut total_added = 0;
        let mut total_skipped = 0;
        let mut empty_pages_count = 0;
        let last_checked_date = tracker.tracked_genres.get(&genre_id)
            .map(|g| g.last_checked_date.clone())
            .unwrap_or_else(|| start_date.to_string());
        
        // Process pages in reverse order (from last_page to 1)
        for current_page in (1..=last_page).rev() {
            println!("\nProcessing page {} of {}", current_page, last_page);
            
            let url = tracker.build_soundeo_url(genre_id, start_date, end_date, current_page);
            
            // Get tracks from this page
            let mut track_list = SoundeoTracksList::new(url.clone()).change_context(GenreTrackerError)?;
            track_list
                .get_tracks_id(&soundeo_user)
                .await
                .change_context(GenreTrackerError)?;
            
            if track_list.track_ids.is_empty() {
                println!("No tracks found on page {}", current_page);
                continue;
            }
            
            println!("Found {} tracks on page {}", track_list.track_ids.len(), current_page);
            
            // Filter tracks by date and get track info
            let mut tracks_to_process = HashSet::new();
            let mut most_recent_date_in_page = None;
            let mut tracks_processed_count = 0;
            
            for track_id in &track_list.track_ids {
                let mut track_info = SoundeoTrack::new(track_id.clone());
                track_info
                    .get_info(&soundeo_user, false) // Don't print for each track
                    .await
                    .change_context(GenreTrackerError)?;
                
                // Check if track date is >= last_checked_date (include same day)
                if track_info.date >= last_checked_date {
                    tracks_to_process.insert(track_id.clone());
                    tracks_processed_count += 1;
                    
                    // Update most recent date in this page
                    if most_recent_date_in_page.is_none() || track_info.date > *most_recent_date_in_page.as_ref().unwrap() {
                        most_recent_date_in_page = Some(track_info.date.clone());
                    }
                }
            }
            
            if tracks_to_process.is_empty() {
                println!("No new tracks to process on page {} (all tracks are older than {})", current_page, last_checked_date);
                empty_pages_count += 1;
                
                // Stop if we've had several empty pages
                if empty_pages_count >= 3 {
                    println!("Stopping early - found {} consecutive pages with no new tracks", empty_pages_count);
                    break;
                }
                continue;
            }
            
            empty_pages_count = 0; // Reset counter
            
            // Process tracks using the common processor
            let genre_name = tracker.available_genres.get(&genre_id)
                .map(|info| info.name.as_str())
                .unwrap_or("Unknown Genre");
            
            let context_description = format!("from {} (page {})", genre_name, current_page);
            
            let (added, skipped) = TrackProcessor::process_tracks_to_queue(
                &tracks_to_process,
                &soundeo_user,
                Priority::Normal,
                false, // repeat_download = false for genre tracking
                &context_description,
            )
            .await
            .change_context(GenreTrackerError)?;
            
            total_added += added;
            total_skipped += skipped;
            
            // Update progress after each page
            if let Some(most_recent_date) = most_recent_date_in_page {
                tracker.update_last_checked(genre_id).change_context(GenreTrackerError)?;
                if let Some(tracked_genre) = tracker.tracked_genres.get_mut(&genre_id) {
                    tracked_genre.last_checked_date = most_recent_date;
                }
                DjWizardLog::save_genre_tracker(tracker.clone()).change_context(GenreTrackerError)?;
                println!("Progress saved - last checked date updated to: {}", 
                    tracker.tracked_genres.get(&genre_id).unwrap().last_checked_date.cyan());
            }
        }
        
        println!(
            "\n{}: Added {} tracks to queue, skipped {} tracks",
            "Summary".green(),
            total_added.to_string().cyan(),
            total_skipped.to_string().yellow()
        );
        
        Ok(())
    }
}
