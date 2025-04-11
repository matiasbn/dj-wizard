use std::collections::HashMap;
use std::time::Duration;

use crate::dialoguer::Dialoguer;
use crate::log::DjWizardLog;
use colored::Colorize;
use colorize::AnsiColor;
use error_stack::{FutureExt, IntoReport, Report, ResultExt};
use headless_chrome::protocol::cdp::Target::CreateTarget;
use headless_chrome::types::Bounds;
use headless_chrome::util::Timeout;
use headless_chrome::{Browser, LaunchOptions};
use scraper::Selector;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::spotify::track::SpotifyTrack;
use crate::spotify::{SpotifyError, SpotifyResult};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SpotifyPlaylist {
    pub name: String,
    pub spotify_playlist_id: String,
    pub url: String,
    pub tracks: HashMap<String, SpotifyTrack>,
}

impl SpotifyPlaylist {
    pub fn new(url: String) -> SpotifyResult<Self> {
        let playlist_url = Url::parse(&url)
            .into_report()
            .change_context(SpotifyError)?;
        let mut sections = playlist_url
            .path_segments()
            .ok_or(SpotifyError)
            .into_report()?;
        let path = sections.next().unwrap();
        if path != "playlist" {
            return Err(Report::new(SpotifyError).attach_printable("Url is not a playlist url"));
        }
        Ok(Self {
            name: "".to_string(),
            spotify_playlist_id: sections.next().unwrap().to_string(),
            url,
            tracks: HashMap::new(),
        })
    }

    pub async fn get_playlist_info(&mut self) -> SpotifyResult<()> {
        let launch_options = LaunchOptions {
            idle_browser_timeout: Duration::from_secs(30000),
            ..Default::default()
        };
        let browser = Browser::new(launch_options)
            .ok()
            .ok_or(SpotifyError)
            .into_report()
            .change_context(SpotifyError)?;

        let tab = browser
            .new_tab_with_options(CreateTarget {
                url: self.url.clone(),
                width: Some(2000),
                height: Some(9999),
                browser_context_id: None,
                enable_begin_frame_control: Some(false),
                new_window: None,
                background: None,
            })
            .ok()
            .ok_or(SpotifyError)
            .into_report()
            .change_context(SpotifyError)?;

        println!("Loading the playlist...");
        std::thread::sleep(std::time::Duration::from_secs(20));

        println!("Getting the playlist name...");
        let name_element = tab
            .find_element("h1.Type__TypeElement-sc-goli3j-0.dYGhLW")
            .unwrap();

        let name = name_element.get_inner_text().unwrap();

        println!("The playlist name is {}", name.clone().green());
        self.name = name;

        println!("Getting the playlist tracks...");
        let tracks = tab
            .find_elements("div.h4HgbO_Uu1JYg5UGANeQ.wTUruPetkKdWAR1dd6w4")
            .unwrap();
        let title_selector = "div.Type__TypeElement-sc-goli3j-0.fZDcWX.t_yrXoUO3qGsJS4Y6iXX.standalone-ellipsis-one-line";
        let artists_selector = "span.Type__TypeElement-sc-goli3j-0.bDHxRN.rq2VQ5mb9SDAFWbBIUIn.standalone-ellipsis-one-line";
        let track_id_selector = "a.t_yrXoUO3qGsJS4Y6iXX";
        for element in tracks {
            let title = element
                .find_element(title_selector)
                .unwrap()
                .get_inner_text()
                .unwrap();
            let spotify_track_id = element
                .find_element(track_id_selector)
                .unwrap()
                .get_attributes()
                .unwrap()
                .unwrap()[7]
                .clone()
                .trim_start_matches("/track/")
                .to_string();
            let artists = element
                .find_element(artists_selector)
                .unwrap()
                .get_inner_text()
                .unwrap();
            self.tracks.insert(
                spotify_track_id.clone(),
                SpotifyTrack::new(title.clone(), artists.clone(), spotify_track_id.clone()),
            );
            println!(
                "Adding {} by {} to the playlist data",
                title.clone().yellow(),
                artists.clone().cyan()
            );
        }
        Ok(())
    }

    pub fn prompt_select_playlist(prompt_text: &str) -> SpotifyResult<Self> {
        let mut spotify = DjWizardLog::get_spotify().change_context(SpotifyError)?;
        let playlist_names = spotify
            .playlists
            .values()
            .map(|playlist| playlist.name.clone())
            .collect::<Vec<_>>();
        let selection = Dialoguer::select(prompt_text.to_string(), playlist_names.clone(), None)
            .change_context(SpotifyError)?;
        let playlist = spotify.get_playlist_by_name(playlist_names[selection].clone())?;
        Ok(playlist)
    }
}

#[cfg(test)]
mod tests {
    use crate::spotify::playlist::SpotifyPlaylist;

    use super::*;

    #[tokio::test]
    async fn test_get_playlist() {
        // DnB
        let playlist_url = "https://open.spotify.com/playlist/6YYCPN91F4xI1Z17Hzn7ir".to_string();
        // House
        // let playlist_url = "https://open.spotify.com/playlist/0B2bjiQkVcIHXXgqFb1k7T".to_string();
        let mut playlist = SpotifyPlaylist::new(playlist_url).unwrap();
        playlist.get_playlist_info().await.unwrap();
        // println!("{:#?}", api_response);
    }
}
