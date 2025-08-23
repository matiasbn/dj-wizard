use std::collections::HashMap;
use std::env;

use crate::dialoguer::Dialoguer;
use crate::log::DjWizardLog;
use base64::{engine::general_purpose, Engine as _};
use colored::Colorize;
use dotenvy::dotenv;
use error_stack::{IntoReport, Report, ResultExt};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::spotify::track::SpotifyTrack;
use crate::spotify::{SpotifyError, SpotifyResult};
use crate::user::User;

#[derive(Serialize, Deserialize, Debug)]
struct TokenResponse {
    access_token: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct ApiArtist {
    name: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SpotifyPlaylist {
    pub name: String,
    pub spotify_playlist_id: String,
    pub url: String,
    pub tracks: HashMap<String, SpotifyTrack>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct ApiTrack {
    id: Option<String>,
    name: String,
    artists: Vec<ApiArtist>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct PlaylistItem {
    track: Option<ApiTrack>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct PlaylistTracks {
    items: Vec<PlaylistItem>,
    next: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct ApiPlaylist {
    name: String,
    tracks: PlaylistTracks,
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

    /// Fetches playlist information (name and tracks) from the Spotify API.
    ///
    /// This function requires `SPOTIFY_CLIENT_ID` and `SPOTIFY_CLIENT_SECRET` to be set
    /// in a `.env` file in the project root. It uses the Client Credentials Flow to
    /// authenticate with the Spotify API.
    pub async fn get_playlist_info(
        &mut self,
        user_config: &mut User,
        verbose: bool,
    ) -> SpotifyResult<()> {
        if verbose {
            println!("Getting playlist info from Spotify API...");
        }

        let client = reqwest::Client::new();

        // --- Get Playlist Info (first page) ---
        let playlist_url = format!(
            "https://api.spotify.com/v1/playlists/{}",
            self.spotify_playlist_id
        );

        let mut response = client
            .get(&playlist_url)
            .bearer_auth(&user_config.spotify_access_token)
            .send()
            .await
            .into_report()
            .change_context(SpotifyError)?;

        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            user_config
                .refresh_spotify_token()
                .await
                .change_context(SpotifyError)?;
            response = client
                .get(&playlist_url)
                .bearer_auth(&user_config.spotify_access_token)
                .send()
                .await
                .into_report()
                .change_context(SpotifyError)?;
        }

        if !response.status().is_success() {
            let error_body = response
                .text()
                .await
                .unwrap_or_else(|_| "Could not read error body".to_string());
            return Err(Report::new(SpotifyError)
                .attach_printable(format!("Spotify API returned an error: {}", error_body)));
        }

        let mut api_playlist: ApiPlaylist = response
            .json::<ApiPlaylist>()
            .await
            .into_report()
            .change_context(SpotifyError)?;

        self.name = api_playlist.name;
        if verbose {
            println!("The playlist name is {}", self.name.clone().green());
        }

        // --- Process tracks and handle pagination ---
        self.tracks.clear();
        let mut next_url = api_playlist.tracks.next.take();

        self.process_track_items(api_playlist.tracks.items, verbose);

        while let Some(url) = next_url {
            let mut paginated_response_raw = client
                .get(&url)
                .bearer_auth(&user_config.spotify_access_token)
                .send()
                .await
                .into_report()
                .change_context(SpotifyError)?;

            if paginated_response_raw.status() == reqwest::StatusCode::UNAUTHORIZED {
                user_config
                    .refresh_spotify_token()
                    .await
                    .change_context(SpotifyError)?;
                paginated_response_raw = client
                    .get(&url)
                    .bearer_auth(&user_config.spotify_access_token)
                    .send()
                    .await
                    .into_report()
                    .change_context(SpotifyError)?;
            }

            if !paginated_response_raw.status().is_success() {
                let error_body = paginated_response_raw
                    .text()
                    .await
                    .unwrap_or_else(|_| "Could not read error body".to_string());
                return Err(Report::new(SpotifyError)
                    .attach_printable(format!("Spotify API returned an error: {}", error_body)));
            }

            let paginated_response: PlaylistTracks = paginated_response_raw
                .json::<PlaylistTracks>()
                .await
                .into_report()
                .change_context(SpotifyError)?;

            self.process_track_items(paginated_response.items, verbose);
            next_url = paginated_response.next;
        }

        Ok(())
    }

    fn process_track_items(&mut self, items: Vec<PlaylistItem>, verbose: bool) {
        for item in items {
            if let Some(track) = item.track {
                if let Some(track_id) = track.id {
                    let artists: Vec<String> =
                        track.artists.iter().map(|a| a.name.clone()).collect();
                    let artists_string = artists.join(", ");

                    let spotify_track = SpotifyTrack::new(
                        track.name.clone(),
                        artists_string.clone(),
                        track_id.clone(),
                    );

                    self.tracks.insert(track_id, spotify_track);
                    if verbose {
                        println!(
                            "Adding {} by {} to the playlist data",
                            track.name.yellow(),
                            artists_string.cyan()
                        );
                    }
                } else {
                    println!(
                        "Skipping track '{}' because it has no ID (it might be a local file).",
                        track.name.yellow()
                    );
                }
            }
        }
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
    use dotenvy::dotenv;

    use super::*;

    // #[tokio::test]
    // #[ignore] // Requires .env credentials and network access. Run with `cargo test -- --ignored`
    // async fn test_get_playlist() {
    //     dotenv().ok();
    //     let playlist_url = "https://open.spotify.com/playlist/6YYCPN91F4xI1Z17Hzn7ir".to_string();
    //     let mut playlist = SpotifyPlaylist::new(playlist_url).unwrap();
    //     playlist.get_playlist_info().await.unwrap();
    //     assert!(!playlist.tracks.is_empty());
    //     assert!(!playlist.name.is_empty());
    // }
}
