use crate::spotify::track::SpotifyTrack;
use crate::spotify::{SpotifyError, SpotifyResult};
use error_stack::{FutureExt, IntoReport, ResultExt};
use headless_chrome::protocol::cdp::Target::CreateTarget;
use headless_chrome::types::Bounds;
use headless_chrome::Browser;
use scraper::Selector;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SpotifyPlaylist {
    pub name: String,
    pub url: String,
}

impl SpotifyPlaylist {
    pub fn new(url: String) -> Self {
        Self {
            name: "".to_string(),
            url,
        }
    }

    pub async fn get_playlist(&mut self) -> SpotifyResult<()> {
        let browser = Browser::default()
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

        tab.wait_until_navigated().unwrap();

        std::thread::sleep(std::time::Duration::from_secs(5));

        // let name_element = tab
        //     .find_element("h1.Type__TypeElement-sc-goli3j-0.dYGhLW")
        //     .unwrap();
        //
        // let name = name_element.get_inner_text().unwrap();
        // self.name = name;
        // println!("{}", self.name.clone());

        let tracks = tab
            .find_elements("div.h4HgbO_Uu1JYg5UGANeQ.wTUruPetkKdWAR1dd6w4")
            .unwrap();
        let title_selector = "div.Type__TypeElement-sc-goli3j-0.fZDcWX.t_yrXoUO3qGsJS4Y6iXX.standalone-ellipsis-one-line";
        let artists_selector = "span.Type__TypeElement-sc-goli3j-0.bDHxRN.rq2VQ5mb9SDAFWbBIUIn.standalone-ellipsis-one-line";
        for element in tracks {
            let title_element = element.find_element(title_selector).unwrap();
            let title = title_element.get_inner_text().unwrap();
            let artists_element = element.find_element(artists_selector).unwrap();
            let artists = artists_element.get_inner_text().unwrap();
            let spotify_track = SpotifyTrack::new(title, artists);
            println!("track {:#?}", spotify_track);
        }

        // println!("{:#?}", tab.get_document());
        //
        // // let attribute_name = "data-testid";
        // // let attribute_value = "entityTitle";
        // let attribute_name = "id";
        // let attribute_value = "tophf";
        //
        // // Evaluate JavaScript code to find the span element by attribute value
        // let script = format!(
        //     r#"Array.from(document.querySelectorAll('div')).find(span => span.getAttribute('{}') === '{}')"#,
        //     attribute_name, attribute_value
        // );
        // let result = tab.evaluate(&script, true).unwrap();
        //
        // if let Some(value) = result.value {
        //     let element_html = value.to_string();
        //     println!("Element found:\n{}", element_html);
        // } else {
        //     println!(
        //         "No span element found with attribute value: {}={}",
        //         attribute_name, attribute_value
        //     );
        // }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spotify::playlist::SpotifyPlaylist;

    #[tokio::test]
    async fn test_get_playlist() {
        // DnB
        let playlist_url = "https://open.spotify.com/playlist/6YYCPN91F4xI1Z17Hzn7ir".to_string();
        // House
        // let playlist_url = "https://open.spotify.com/playlist/0B2bjiQkVcIHXXgqFb1k7T".to_string();
        let mut playlist = SpotifyPlaylist::new(playlist_url);
        playlist.get_playlist().await.unwrap();
        // println!("{:#?}", api_response);
    }
}
