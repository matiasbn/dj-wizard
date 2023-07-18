use crate::spotify::{SpotifyError, SpotifyResult};
use error_stack::{FutureExt, IntoReport, ResultExt};
use headless_chrome::Browser;
use serde::{Deserialize, Serialize};

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
            .new_tab()
            .ok()
            .ok_or(SpotifyError)
            .into_report()
            .change_context(SpotifyError)?;

        tab.navigate_to(&self.url)
            .ok()
            .ok_or(SpotifyError)
            .into_report()
            .change_context(SpotifyError)?
            .wait_until_navigated()
            .ok()
            .ok_or(SpotifyError)
            .into_report()
            .change_context(SpotifyError)?;

        let name_element = tab
            .find_element("h1.Type__TypeElement-sc-goli3j-0.dYGhLW")
            .unwrap();

        let name = name_element.get_inner_text().unwrap();
        self.name = name;
        println!("{}", self.name.clone());

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
        let playlist_url = "https://open.spotify.com/playlist/5XGbuIRSb5INv66b817DJH".to_string();
        let mut playlist = SpotifyPlaylist::new(playlist_url);
        playlist.get_playlist().await.unwrap();
        // println!("{:#?}", api_response);
    }
}
