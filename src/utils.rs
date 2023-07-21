use crate::soundeo::full_info::SoundeoTrackFullInfo;
use crate::soundeo::track::SoundeoTrack;
use crate::soundeo_log::DjWizardLog;
use crate::user::SoundeoUser;
use crate::{DjWizardError, DjWizardResult};
use colored::Colorize;
use colorize::AnsiColor;
use error_stack::{IntoReport, ResultExt};

pub async fn download_track_and_update_log(
    mut soundeo_user: &mut SoundeoUser,
    soundeo_log: &mut DjWizardLog,
    mut track_id: &String,
) -> DjWizardResult<()> {
    let mut track_info = SoundeoTrackFullInfo::new(track_id.clone());
    track_info
        .get_info(&soundeo_user)
        .await
        .change_context(DjWizardError)?;
    // validate if we have can download tracks
    soundeo_user
        .validate_remaining_downloads()
        .change_context(DjWizardError)?;
    if soundeo_log.downloaded_tracks.contains_key(track_id) {
        println!(
            "Track already downloaded: {},  {}",
            track_info.title, track_info.track_url
        );
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

pub async fn downloaded_tracks_to_soundeo_tracks() -> DjWizardResult<()> {
    let mut soundeo_user = SoundeoUser::new().change_context(DjWizardError)?;
    soundeo_user
        .login_and_update_user_info()
        .await
        .change_context(DjWizardError)?;
    let downloaded_tracks = DjWizardLog::read_log()
        .change_context(DjWizardError)?
        .downloaded_tracks;
    let mut wlog = DjWizardLog::read_log().change_context(DjWizardError)?;
    let pending_to_update = downloaded_tracks
        .into_iter()
        .filter_map(|track| {
            if !wlog.soundeo.tracks_info.contains_key(&track.0) {
                Some(track.0.clone())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    let pending_to_update_len = pending_to_update.len();
    for (dt_index, dt_id) in pending_to_update.into_iter().enumerate() {
        println!(
            "Updating {} of {} tracks",
            format!("{}", dt_index + 1).cyan(),
            format!("{}", pending_to_update_len).cyan(),
        );
        let mut log = DjWizardLog::read_log().change_context(DjWizardError)?;
        if log.soundeo.tracks_info.get(&dt_id).is_some() {
            println!("Track already stored: {}", dt_id.clone().yellow());
            continue;
        }
        let mut full_info = SoundeoTrackFullInfo::new(dt_id.clone());
        full_info
            .get_info(&soundeo_user)
            .await
            .change_context(DjWizardError)?;
        full_info.already_downloaded = true;
        log.soundeo.tracks_info.insert(dt_id.clone(), full_info);
        log.save_log(&soundeo_user).change_context(DjWizardError)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_parse_downloaded_track() -> DjWizardResult<()> {
        downloaded_tracks_to_soundeo_tracks().await?;
        Ok(())
    }
}
