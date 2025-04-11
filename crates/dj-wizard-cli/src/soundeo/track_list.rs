use std::cmp::min;
use std::collections::HashSet;
use std::fmt;
use std::fs::File;
use std::io::Write;
use std::str::FromStr;

use colored::Colorize;
use colorize::AnsiColor;
use error_stack::{IntoReport, Report, ResultExt};
use futures_util::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use lazy_regex::regex;
use reqwest::Client;
use scraper::{ElementRef, Html, Selector};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use url::Url;

use crate::soundeo::api::SoundeoAPI;
use crate::soundeo::{SoundeoError, SoundeoResult};
use crate::user::SoundeoUser;
use crate::Suggestion;

#[derive(Debug)]
pub struct SoundeoTracksList {
    pub url: String,
    pub track_ids: HashSet<String>,
}

impl SoundeoTracksList {
    pub fn new(url: String) -> SoundeoResult<Self> {
        Ok(Self {
            url,
            track_ids: HashSet::new(),
        })
    }

    pub async fn get_tracks_id(&mut self, user: &SoundeoUser) -> SoundeoResult<()> {
        let retrieved_page = self.retrieve_html(&user, self.url.clone()).await?;
        let page_body = Html::parse_document(&retrieved_page);
        let track_download_link_element = Selector::parse("a.track-download-lnk").unwrap();
        for track_element in page_body.select(&track_download_link_element) {
            let track_id = track_element
                .value()
                .attr("data-track-id")
                .ok_or(SoundeoError)
                .into_report()?
                .to_string();
            self.track_ids.insert(track_id);
        }
        Ok(())
    }

    async fn retrieve_html(
        &self,
        soundeo_user: &SoundeoUser,
        url: String,
    ) -> SoundeoResult<String> {
        let client = Client::new();
        let session_cookie = soundeo_user
            .get_session_cookie()
            .change_context(SoundeoError)?;
        let response = client
            .get(url.clone())
            .header("cookie", session_cookie)
            .send()
            .await
            .into_report()
            .change_context(SoundeoError)?
            .text()
            .await
            .into_report()
            .change_context(SoundeoError)?;
        Ok(response)
    }
}
