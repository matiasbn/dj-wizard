use crate::soundeo::track::SoundeoTrack;
use crate::soundeo_log::DjWizardLog;
use crate::user::SoundeoUser;
use crate::{DjWizardError, DjWizardResult};
use colorize::AnsiColor;
use error_stack::ResultExt;

pub async fn download_track_and_update_log(
    mut soundeo_user: &mut SoundeoUser,
    soundeo_log: &mut DjWizardLog,
    mut track_id: &String,
) -> DjWizardResult<()> {
    // validate if we have can download tracks
    soundeo_user
        .validate_remaining_downloads()
        .change_context(DjWizardError)?;
    if soundeo_log.downloaded_tracks.contains_key(track_id) {
        println!("Track already downloaded: {}", track_id.clone());
        return Ok(());
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
    Ok(())
}
