use std::collections::HashSet;

use colored::Colorize;
use error_stack::ResultExt;

use crate::log::{DjWizardLog, Priority};
use crate::queue::{QueueError, QueueResult};
use crate::soundeo::track::SoundeoTrack;
use crate::user::SoundeoUser;

pub struct TrackProcessor;

impl TrackProcessor {
    /// Processes a collection of track IDs and adds them to the queue
    /// Shows detailed progress for each track similar to the existing queue functionality
    pub async fn process_tracks_to_queue(
        track_ids: &HashSet<String>,
        soundeo_user: &SoundeoUser,
        priority: Priority,
        repeat_download: bool,
        context_description: &str, // e.g., "from Drum and Bass genre", "from playlist"
    ) -> QueueResult<(usize, usize)> {
        let available_tracks = DjWizardLog::get_available_tracks().change_context(QueueError)?;
        let queued_tracks = DjWizardLog::get_queued_tracks().change_context(QueueError)?;
        let queued_ids: HashSet<String> = queued_tracks.iter().map(|t| t.track_id.clone()).collect();
        let soundeo_info = DjWizardLog::get_soundeo().change_context(QueueError)?;

        let total_tracks = track_ids.len();
        let mut total_added = 0;
        let mut total_skipped = 0;

        println!(
            "Processing {} tracks {}",
            total_tracks.to_string().cyan(),
            context_description
        );

        for (track_id_index, track_id) in track_ids.iter().enumerate() {
            println!(
                "-----------------------------------------------------------------------------"
            );

            println!(
                "Processing track {} of {}",
                (track_id_index + 1).to_string().cyan(),
                total_tracks.to_string().cyan()
            );

            // Skip if already queued (quick check before getting track info)
            if queued_ids.contains(track_id) {
                // Get track info to show title and URL
                let mut track_info = SoundeoTrack::new(track_id.clone());
                track_info
                    .get_info(soundeo_user, false)
                    .await
                    .change_context(QueueError)?;
                println!(
                    "Track with id {} was previously queued, skipping: {}, {}",
                    track_id.clone().yellow(),
                    track_info.title.yellow(),
                    track_info.get_track_url().yellow()
                );
                total_skipped += 1;
                continue;
            }

            // Skip if already available (quick check before getting track info)
            if available_tracks.contains(track_id) {
                println!(
                    "Track with id {} is already available for download, skipping",
                    track_id.clone().yellow(),
                );
                total_skipped += 1;
                continue;
            }

            // Get detailed track info
            let mut track_info = SoundeoTrack::new(track_id.clone());
            track_info
                .get_info(soundeo_user, true)
                .await
                .change_context(QueueError)?;

            // Check if already downloaded
            if track_info.already_downloaded {
                if !repeat_download {
                    track_info.print_already_downloaded();
                    total_skipped += 1;
                    continue;
                } else {
                    track_info.print_downloading_again();
                    // Note: We don't reset already_downloaded here as it's expensive
                    // The reset will happen during actual download
                }
            }

            // Add to queue
            let queue_result = DjWizardLog::add_queued_track(track_id.clone(), priority)
                .change_context(QueueError)?;
            
            if queue_result {
                println!(
                    "Track {} successfully queued: {}",
                    track_info.title.green(),
                    track_info.get_track_url().green()
                );
                total_added += 1;
            } else {
                println!(
                    "Track {} was previously queued, skipping: {}",
                    track_info.title.yellow(),
                    track_info.get_track_url().yellow()
                );
                total_skipped += 1;
            }
        }

        println!(
            "\n{}: Added {} tracks to queue, skipped {} tracks",
            "Summary".green(),
            total_added.to_string().cyan(),
            total_skipped.to_string().yellow()
        );

        Ok((total_added, total_skipped))
    }
}