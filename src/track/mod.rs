pub mod full_info;

use crate::user::SoundeoUser;
use indicatif::{ProgressBar, ProgressStyle};
use serde::{Deserialize, Serialize};
use std::cmp::min;
use std::collections::HashSet;

use crate::Suggestion;
use colored::Colorize;
use colorize::AnsiColor;
use error_stack::{IntoReport, Report, ResultExt};
use futures_util::StreamExt;
use lazy_regex::regex;
use reqwest::Client;
use scraper::{ElementRef, Html, Selector};
use serde_json::{json, Value};
use std::fmt;
use std::fs::File;
use std::io::Write;
use std::str::FromStr;
use url::Url;

#[derive(Debug)]
pub struct SoundeoTrackError;

impl fmt::Display for SoundeoTrackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SoundeoTrack error")
    }
}

impl std::error::Error for SoundeoTrackError {}

pub type SoundeoTrackResult<T> = error_stack::Result<T, SoundeoTrackError>;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SoundeoTrack {
    pub track_id: String,
    pub download_url: String,
    pub file_name: String,
    pub total_size: u64,
    pub downloaded: bool,
}

impl SoundeoTrack {
    pub async fn new(track_id: String) -> SoundeoTrackResult<Self> {
        let mut new_track = Self {
            track_id,
            download_url: "".to_string(),
            file_name: "".to_string(),
            total_size: 0,
            downloaded: false,
        };
        // new_track.get_name_and_size().await?;
        Ok(new_track)
    }

    async fn get_name_and_size(&mut self) -> SoundeoTrackResult<()> {
        let client = reqwest::Client::new();
        let response = client
            .get(&self.download_url)
            .send()
            .await
            .into_report()
            .change_context(SoundeoTrackError)?;
        let total_size = response.content_length().unwrap();
        self.total_size = total_size;
        let file_name = response
            .headers()
            .get("content-disposition")
            .ok_or(SoundeoTrackError)
            .into_report()
            .change_context(SoundeoTrackError)?
            .to_str()
            .into_report()
            .change_context(SoundeoTrackError)?
            .trim_start_matches("attachment; filename=\"")
            .trim_end_matches("\"")
            .to_string();
        self.file_name = file_name.clone();
        Ok(())
    }

    async fn get_download_url(&mut self, soundeo_user: &mut SoundeoUser) -> SoundeoTrackResult<()> {
        let client = Client::new();
        let url = format!("https://soundeo.com/download/{}/3", self.track_id);
        let cookie = format!(
            "{}; {}; {}; {}; {}",
            soundeo_user.pk_id,
            soundeo_user.pk_ses,
            soundeo_user.snd,
            soundeo_user.snd_data,
            soundeo_user.bruid
        );
        let response = client
            .get(url)
            .header("authority", "soundeo.com")
            .header("accept", "application/json, text/javascript, */*; q=0.01")
            .header("accept-language", "en-US,en;q=0.9")
            .header("content-type", "application/x-www-form-urlencoded; charset=UTF-8")
            .header("cookie", cookie)
            .header("referer", "https://soundeo.com/track/peggy-gou-it-goes-like-nanana-original-mix-17184136.html")
            .header("sec-ch-ua", r#"Not.A/Brand";v="8", "Chromium";v="114", "Brave";v="114"#)
            .header("sec-ch-ua-mobile", "?0")
            .header("sec-ch-ua-platform", "macOS")
            .header("sec-fetch-dest", "empty")
            .header("sec-fetch-mode", "cors")
            .header("sec-fetch-site", "same-origin")
            .header("sec-gpc", "1")
            .header("user-agent", "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/114.0.0.0 Safari/537.36")
            .header("x-requested-with", "XMLHttpRequest")
            .send()
            .await.into_report().change_context(SoundeoTrackError)?;
        let response_text = response
            .text()
            .await
            .into_report()
            .change_context(SoundeoTrackError)?;
        let json_resp: Value = serde_json::from_str(&response_text)
            .into_report()
            .change_context(SoundeoTrackError)?;
        let download_url = json_resp["jsActions"]["redirect"]["url"]
            .clone()
            .to_string();
        self.download_url = download_url
            .trim_end_matches("\"")
            .trim_start_matches("\"")
            .to_string();
        self.get_name_and_size().await?;
        soundeo_user
            .login_and_update_user_info()
            .await
            .change_context(SoundeoTrackError)?;
        Ok(())
    }

    pub async fn download_track(
        &mut self,
        soundeo_user: &mut SoundeoUser,
    ) -> SoundeoTrackResult<()> {
        self.get_download_url(soundeo_user).await?;
        if soundeo_user.remaining_downloads_bonus == "0".to_string() {
            println!(
                "{} tracks before reaching the download limit",
                soundeo_user.remaining_downloads.clone().cyan()
            );
        } else {
            println!(
                "{} (plus {} bonus) tracks before reaching the download limit",
                soundeo_user.remaining_downloads.clone().cyan(),
                soundeo_user.remaining_downloads_bonus.clone().cyan(),
            );
        }
        let pb = ProgressBar::new(self.total_size);
        pb.set_style(ProgressStyle::default_bar()
            .template("{msg}\n{spinner:.green} [{elapsed_precise}] [{wide_bar:.white/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta})").into_report().change_context(SoundeoTrackError)?
            .progress_chars("█  "));
        pb.set_message(format!(
            "Downloading {}, track id: {}",
            self.file_name.clone().cyan(),
            self.track_id.clone().cyan()
        ));

        let mut downloaded: u64 = 0;
        let client = reqwest::Client::new();
        let response = client
            .get(&self.download_url)
            .send()
            .await
            .into_report()
            .change_context(SoundeoTrackError)?;
        let mut stream = response.bytes_stream();
        let file_path = format!("{}/{}", soundeo_user.download_path, self.file_name);
        let mut dest = File::create(file_path.clone())
            .into_report()
            .change_context(SoundeoTrackError)?;

        while let Some(item) = stream.next().await {
            let chunk = item.into_report().change_context(SoundeoTrackError)?;
            dest.write(&chunk)
                .into_report()
                .change_context(SoundeoTrackError)?;
            let new = min(downloaded + (chunk.len() as u64), self.total_size);
            downloaded = new;
            pb.set_position(new);
        }
        self.downloaded = true;
        let message = format!("{} successfully downloaded", self.file_name.clone().green(),);
        pb.finish_with_message(message);
        Ok(())
    }
}

#[derive(Debug)]
pub struct SoundeoTracksList {
    pub url: String,
    pub track_ids: HashSet<String>,
}

impl SoundeoTracksList {
    pub fn new(url: String) -> SoundeoTrackResult<Self> {
        Ok(Self {
            url,
            track_ids: HashSet::new(),
        })
    }

    pub async fn get_tracks_id(&mut self, user: &SoundeoUser) -> SoundeoTrackResult<()> {
        let retrieved_page = self.retrieve_html(&user, self.url.clone()).await?;
        let page_body = Html::parse_document(&retrieved_page);
        let track_download_link_element = Selector::parse("a.track-download-lnk").unwrap();
        for track_element in page_body.select(&track_download_link_element) {
            let track_id = track_element
                .value()
                .attr("data-track-id")
                .ok_or(SoundeoTrackError)
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
    ) -> SoundeoTrackResult<String> {
        let client = Client::new();
        let session_cookie = soundeo_user
            .get_session_cookie()
            .change_context(SoundeoTrackError)?;
        let response = client
            .get(url.clone())
            .header("cookie", session_cookie)
            .send()
            .await
            .into_report()
            .change_context(SoundeoTrackError)?
            .text()
            .await
            .into_report()
            .change_context(SoundeoTrackError)?;
        Ok(response)
    }
}
