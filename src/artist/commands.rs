use colored::Colorize;
use error_stack::ResultExt;
use inflector::Inflector;
use strum::IntoEnumIterator;

use crate::artist::{ArtistCRUD, ArtistError, ArtistManager, ArtistResult};
use crate::dialoguer::Dialoguer;
use crate::log::DjWizardLog;

#[derive(Debug, Clone, strum_macros::Display, strum_macros::EnumIter)]
pub enum ArtistCommands {
    AddFavoriteArtist,
    ViewFavoriteArtists,
    RemoveFavoriteArtist,
    SearchArtists,
    ViewArtistsByGenre,
}

impl ArtistCommands {
    pub fn execute() -> ArtistResult<()> {
        let options = Self::get_options();
        let selection = Dialoguer::select(
            "Artist Manager - What would you like to do?".to_string(),
            options,
            None,
        )
        .change_context(ArtistError)?;

        match Self::get_selection(selection) {
            ArtistCommands::AddFavoriteArtist => Self::add_favorite_artist(),
            ArtistCommands::ViewFavoriteArtists => Self::view_favorite_artists(),
            ArtistCommands::RemoveFavoriteArtist => Self::remove_favorite_artist(),
            ArtistCommands::SearchArtists => Self::search_artists(),
            ArtistCommands::ViewArtistsByGenre => Self::view_artists_by_genre(),
        }
    }

    fn get_options() -> Vec<String> {
        Self::iter()
            .map(|element| element.to_string().to_sentence_case())
            .collect()
    }

    fn get_selection(selection: usize) -> Self {
        Self::iter().nth(selection).unwrap()
    }

    fn add_favorite_artist() -> ArtistResult<()> {
        let mut manager = DjWizardLog::get_artist_manager().change_context(ArtistError)?;

        println!("{}", "Add Favorite Artists".cyan().bold());
        println!("{}", "===================".cyan());

        loop {
            // Get artist name(s) from user
            let artist_name_input = Dialoguer::input(
                "Enter artist name(s) (separate multiple artists with commas, or press Enter to finish):".to_string(),
            )
            .change_context(ArtistError)?;

            if artist_name_input.trim().is_empty() {
                break;
            }

            // Split by comma and process each artist
            let artist_names: Vec<String> = artist_name_input
                .split(',')
                .map(|artist| artist.trim())
                .filter(|artist| !artist.is_empty())
                .map(|artist| ArtistManager::format_artist_name(artist))
                .collect();

            if artist_names.is_empty() {
                println!("No valid artist names entered.");
                continue;
            }

            // Show formatted artists to user
            if artist_names.len() == 1 {
                println!("Found 1 artist: {}", artist_names[0].green());
            } else {
                println!("Found {} artists:", artist_names.len().to_string().cyan());
                for (i, artist) in artist_names.iter().enumerate() {
                    println!("  {}. {}", i + 1, artist.green());
                }
            }

            // Ask for optional genre association
            let genre_input = Dialoguer::input(
                "Enter genre to associate with these artists (optional, press Enter to skip):"
                    .to_string(),
            )
            .change_context(ArtistError)?;

            let genre = if genre_input.trim().is_empty() {
                None
            } else {
                Some(ArtistManager::format_artist_name(&genre_input))
            };

            // Confirm with user
            let confirmation_msg = if let Some(ref g) = genre {
                if artist_names.len() == 1 {
                    format!(
                        "Add '{}' as favorite artist in genre '{}'?",
                        artist_names[0].green(),
                        g.cyan()
                    )
                } else {
                    format!(
                        "Add all these artists as favorites in genre '{}'?",
                        g.cyan()
                    )
                }
            } else {
                if artist_names.len() == 1 {
                    format!("Add '{}' as favorite artist?", artist_names[0].green())
                } else {
                    "Add all these artists as favorites?".to_string()
                }
            };

            let confirmation =
                Dialoguer::confirm(confirmation_msg, Some(true)).change_context(ArtistError)?;

            if confirmation {
                let mut added_count = 0;
                let mut updated_count = 0;
                let mut skipped_count = 0;

                for artist_name in artist_names {
                    match manager.add_artist(&artist_name, genre.as_deref()) {
                        Ok(true) => {
                            if manager.get_artist(&artist_name).unwrap().created_at
                                == manager.get_artist(&artist_name).unwrap().last_updated
                            {
                                println!("Artist '{}' added successfully!", artist_name.green());
                                added_count += 1;
                            } else {
                                println!(
                                    "Artist '{}' updated with new genre!",
                                    artist_name.green()
                                );
                                updated_count += 1;
                            }

                            // Save immediately after adding each artist
                            DjWizardLog::save_artist_manager(manager.clone())
                                .change_context(ArtistError)?;
                        }
                        Ok(false) => {
                            println!(
                                "Artist '{}' already exists with this genre!",
                                artist_name.yellow()
                            );
                            skipped_count += 1;
                        }
                        Err(_) => {
                            println!("Failed to add artist '{}'", artist_name.red());
                            skipped_count += 1;
                        }
                    }
                }

                // Summary
                if added_count > 0 || updated_count > 0 || skipped_count > 0 {
                    println!(
                        "Summary: {} artists added, {} artists updated, {} artists skipped",
                        added_count.to_string().green(),
                        updated_count.to_string().blue(),
                        skipped_count.to_string().yellow()
                    );
                }
            } else {
                println!("Artists not added.");
            }

            // Ask if user wants to add more artists
            let add_another = Dialoguer::confirm("Add more artists?".to_string(), Some(true))
                .change_context(ArtistError)?;

            if !add_another {
                break;
            }
        }

        Ok(())
    }

    fn view_favorite_artists() -> ArtistResult<()> {
        let manager = DjWizardLog::get_artist_manager().change_context(ArtistError)?;

        if manager.favorite_artists.is_empty() {
            println!("{}", "No favorite artists found.".yellow());
            return Ok(());
        }

        println!("{}", "Favorite Artists".cyan().bold());
        println!("{}", "================".cyan());

        let mut artists: Vec<_> = manager.get_all_artists();
        artists.sort_by(|a, b| a.name.cmp(&b.name));

        for (i, artist) in artists.iter().enumerate() {
            println!("\n{}. {}", i + 1, artist.name.green().bold());

            if artist.genres.is_empty() {
                println!("   {}: {}", "Genres".cyan(), "None".yellow());
            } else {
                println!(
                    "   {}: {}",
                    "Genres".cyan(),
                    artist.genres.join(", ").blue()
                );
            }

            println!("   {}: {}", "Added".cyan(), artist.created_at.white());
            if artist.created_at != artist.last_updated {
                println!("   {}: {}", "Updated".cyan(), artist.last_updated.white());
            }
        }

        println!(
            "\n{}: {}",
            "Total artists".green(),
            artists.len().to_string().cyan()
        );

        Ok(())
    }

    fn remove_favorite_artist() -> ArtistResult<()> {
        let mut manager = DjWizardLog::get_artist_manager().change_context(ArtistError)?;

        if manager.favorite_artists.is_empty() {
            println!("{}", "No favorite artists found.".yellow());
            return Ok(());
        }

        let artists: Vec<_> = manager.get_all_artists();
        let mut sorted_artists = artists;
        sorted_artists.sort_by(|a, b| a.name.cmp(&b.name));

        // Select artist to remove
        let artist_options: Vec<String> = sorted_artists
            .iter()
            .map(|artist| {
                if artist.genres.is_empty() {
                    artist.name.clone()
                } else {
                    format!("{} ({})", artist.name, artist.genres.join(", "))
                }
            })
            .collect();

        let mut options_with_cancel = artist_options.clone();
        options_with_cancel.push("Cancel".to_string());

        let selection = Dialoguer::select(
            "Select an artist to remove:".to_string(),
            options_with_cancel,
            None,
        )
        .change_context(ArtistError)?;

        // If user selected "Cancel"
        if selection == artist_options.len() {
            return Ok(());
        }

        let selected_artist_name = sorted_artists[selection].name.clone();
        let selected_artist_genres = sorted_artists[selection].genres.clone();

        // If artist has multiple genres, ask if they want to remove from specific genre or completely
        if selected_artist_genres.len() > 1 {
            let removal_options = vec![
                "Remove from specific genre".to_string(),
                "Remove artist completely".to_string(),
                "Cancel".to_string(),
            ];

            let removal_choice = Dialoguer::select(
                format!(
                    "'{}' is associated with multiple genres. What would you like to do?",
                    selected_artist_name.green()
                ),
                removal_options,
                None,
            )
            .change_context(ArtistError)?;

            match removal_choice {
                0 => {
                    // Remove from specific genre
                    let genre_options: Vec<String> = selected_artist_genres.clone();
                    let genre_selection = Dialoguer::select(
                        "Select genre to remove artist from:".to_string(),
                        genre_options.clone(),
                        None,
                    )
                    .change_context(ArtistError)?;

                    let genre_to_remove = &genre_options[genre_selection];

                    let confirmation = Dialoguer::confirm(
                        format!(
                            "Remove '{}' from genre '{}'?",
                            selected_artist_name.red(),
                            genre_to_remove.cyan()
                        ),
                        Some(false),
                    )
                    .change_context(ArtistError)?;

                    if confirmation {
                        manager.remove_artist_from_genre(&selected_artist_name, genre_to_remove)?;
                        println!(
                            "Artist '{}' removed from genre '{}'!",
                            selected_artist_name.red(),
                            genre_to_remove.cyan()
                        );

                        // Save changes
                        DjWizardLog::save_artist_manager(manager).change_context(ArtistError)?;
                    }
                }
                1 => {
                    // Remove artist completely
                    let confirmation = Dialoguer::confirm(
                        format!(
                            "Remove '{}' completely from all genres?",
                            selected_artist_name.red()
                        ),
                        Some(false),
                    )
                    .change_context(ArtistError)?;

                    if confirmation {
                        manager.remove_artist(&selected_artist_name)?;
                        println!(
                            "Artist '{}' removed completely!",
                            selected_artist_name.red()
                        );

                        // Save changes
                        DjWizardLog::save_artist_manager(manager).change_context(ArtistError)?;
                    }
                }
                _ => {} // Cancel
            }
        } else {
            // Artist has 0 or 1 genre, remove completely
            let confirmation = Dialoguer::confirm(
                format!(
                    "Remove '{}' from favorite artists?",
                    selected_artist_name.red()
                ),
                Some(false),
            )
            .change_context(ArtistError)?;

            if confirmation {
                manager.remove_artist(&selected_artist_name)?;
                println!(
                    "Artist '{}' removed successfully!",
                    selected_artist_name.red()
                );

                // Save changes
                DjWizardLog::save_artist_manager(manager).change_context(ArtistError)?;
            }
        }

        Ok(())
    }

    fn search_artists() -> ArtistResult<()> {
        let manager = DjWizardLog::get_artist_manager().change_context(ArtistError)?;

        if manager.favorite_artists.is_empty() {
            println!("{}", "No favorite artists found.".yellow());
            return Ok(());
        }

        let query =
            Dialoguer::input("Enter search term:".to_string()).change_context(ArtistError)?;

        if query.trim().is_empty() {
            return Ok(());
        }

        let results = manager.search_artists(&query);

        if results.is_empty() {
            println!("No artists found matching '{}'", query.yellow());
            return Ok(());
        }

        println!(
            "Found {} artist(s) matching '{}':",
            results.len().to_string().cyan(),
            query.green()
        );
        println!("{}", "=".repeat(40).cyan());

        for (i, artist) in results.iter().enumerate() {
            println!("\n{}. {}", i + 1, artist.name.green().bold());

            if artist.genres.is_empty() {
                println!("   {}: {}", "Genres".cyan(), "None".yellow());
            } else {
                println!(
                    "   {}: {}",
                    "Genres".cyan(),
                    artist.genres.join(", ").blue()
                );
            }
        }

        Ok(())
    }

    fn view_artists_by_genre() -> ArtistResult<()> {
        let manager = DjWizardLog::get_artist_manager().change_context(ArtistError)?;

        if manager.favorite_artists.is_empty() {
            println!("{}", "No favorite artists found.".yellow());
            return Ok(());
        }

        let genres = manager.get_all_genres();

        if genres.is_empty() {
            println!("{}", "No genres found in favorite artists.".yellow());
            return Ok(());
        }

        let mut genre_options = genres.clone();
        genre_options.push("Show artists without genre".to_string());
        genre_options.push("Cancel".to_string());

        let selection = Dialoguer::select(
            "Select a genre to view artists:".to_string(),
            genre_options.clone(),
            None,
        )
        .change_context(ArtistError)?;

        if selection == genre_options.len() - 1 {
            return Ok(()); // Cancel
        }

        if selection == genre_options.len() - 2 {
            // Show artists without genre
            let artists_without_genre: Vec<_> = manager
                .get_all_artists()
                .into_iter()
                .filter(|artist| artist.genres.is_empty())
                .collect();

            if artists_without_genre.is_empty() {
                println!("{}", "No artists without genre found.".yellow());
            } else {
                println!("{}", "Artists without genre:".cyan().bold());
                println!("{}", "====================".cyan());

                for (i, artist) in artists_without_genre.iter().enumerate() {
                    println!("{}. {}", i + 1, artist.name.green());
                }
            }
        } else {
            // Show artists for selected genre
            let selected_genre = &genres[selection];
            let artists = manager.get_artists_by_genre(selected_genre);

            println!("Artists in genre '{}':", selected_genre.cyan().bold());
            println!("{}", "=".repeat(selected_genre.len() + 18).cyan());

            if artists.is_empty() {
                println!("{}", "No artists found for this genre.".yellow());
            } else {
                for (i, artist) in artists.iter().enumerate() {
                    println!("{}. {}", i + 1, artist.name.green());
                }

                println!(
                    "\n{}: {}",
                    "Total".green(),
                    artists.len().to_string().cyan()
                );
            }
        }

        Ok(())
    }
}
