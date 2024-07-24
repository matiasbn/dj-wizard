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
use crate::soundeo::track::SoundeoTrack;
use crate::soundeo::track_list::SoundeoTracksList;
use crate::soundeo::Soundeo;
use crate::user::SoundeoUser;

use super::{UrlListError, UrlListResult};

#[derive(Debug, Deserialize, Serialize, Clone, strum_macros::Display, strum_macros::EnumIter)]
pub enum UrlListCommands {
    AddToUrlList,
    DownloadFromUrl,
}

impl UrlListCommands {
    pub async fn execute() -> UrlListResult<()> {
        let options = Self::get_options();
        let selection = Dialoguer::select("What you want to do?".to_string(), options, None)
            .change_context(UrlListError)?;
        return match Self::get_selection(selection) {
            UrlListCommands::AddToUrlList => Self::add_to_url_list().await,
            UrlListCommands::DownloadFromUrl => Self::download_from_url().await,
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

    async fn add_to_url_list() -> UrlListResult<()> {
        Ok(())
    }

    async fn download_from_url() -> UrlListResult<()> {
        let prompt_text = format!("Soundeo url: ");
        let url = Dialoguer::input(prompt_text).change_context(UrlListError)?;
        let soundeo_url = Url::parse(&url)
            .into_report()
            .change_context(UrlListError)?;
        let mut soundeo_user = SoundeoUser::new().change_context(UrlListError)?;
        soundeo_user
            .login_and_update_user_info()
            .await
            .change_context(UrlListError)?;
        let mut track_list =
            SoundeoTracksList::new(soundeo_url.to_string()).change_context(UrlListError)?;
        track_list
            .get_tracks_id(&soundeo_user)
            .await
            .change_context(UrlListError)?;
        // Add all tracks to collection by
        for (_, track_id) in track_list.track_ids.into_iter().enumerate() {
            let mut track = SoundeoTrack::new(track_id);
            track
                .download_track(&mut soundeo_user, true)
                .await
                .change_context(UrlListError)?;
        }
        Ok(())
    }

    fn filter_queue() -> UrlListResult<Vec<String>> {
        let Soundeo { tracks_info } = DjWizardLog::get_soundeo().change_context(UrlListError)?;
        // let tracks = soundeo
        let queued_tracks = DjWizardLog::get_queued_tracks().change_context(UrlListError)?;
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
            .change_context(UrlListError)?;
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
