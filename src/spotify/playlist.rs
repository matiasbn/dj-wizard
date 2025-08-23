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
    pub async fn get_playlist_info(&mut self) -> SpotifyResult<()> {
        println!("Getting playlist info from Spotify API...");
        dotenv().ok();

        let client_id = env::var("SPOTIFY_CLIENT_ID")
            .into_report()
            .change_context(SpotifyError)
            .attach_printable("SPOTIFY_CLIENT_ID environment variable not set. Please create a .env file with the credentials.")?;
        let client_secret = env::var("SPOTIFY_CLIENT_SECRET")
            .into_report()
            .change_context(SpotifyError)
            .attach_printable("SPOTIFY_CLIENT_SECRET environment variable not set. Please create a .env file with the credentials.")?;

        // --- Get Access Token ---
        let client = reqwest::Client::new();
        let auth_string = format!("{}:{}", client_id, client_secret);
        let encoded_auth = general_purpose::STANDARD.encode(auth_string);

        let token_response = client
            .post("https://accounts.spotify.com/api/token")
            .header("Authorization", format!("Basic {}", encoded_auth))
            .form(&[("grant_type", "client_credentials")])
            .send()
            .await
            .into_report()
            .change_context(SpotifyError)?
            .json::<TokenResponse>()
            .await
            .into_report()
            .change_context(SpotifyError)?;
        let access_token = token_response.access_token;

        // --- Get Playlist Info (first page) ---
        let playlist_url = format!(
            "https://api.spotify.com/v1/playlists/{}",
            self.spotify_playlist_id
        );

        let mut api_playlist: ApiPlaylist = client
            .get(&playlist_url)
            .bearer_auth(&access_token)
            .send()
            .await
            .into_report()
            .change_context(SpotifyError)?
            .json::<ApiPlaylist>()
            .await
            .into_report()
            .change_context(SpotifyError)?;

        self.name = api_playlist.name;
        println!("The playlist name is {}", self.name.clone().green());

        // --- Process tracks and handle pagination ---
        self.tracks.clear();
        let mut next_url = api_playlist.tracks.next.take();

        self.process_track_items(api_playlist.tracks.items);

        while let Some(url) = next_url {
            let paginated_response: PlaylistTracks = client
                .get(&url)
                .bearer_auth(&access_token)
                .send()
                .await
                .into_report()
                .change_context(SpotifyError)?
                .json::<PlaylistTracks>()
                .await
                .into_report()
                .change_context(SpotifyError)?;

            self.process_track_items(paginated_response.items);
            next_url = paginated_response.next;
        }

        Ok(())
    }

    fn process_track_items(&mut self, items: Vec<PlaylistItem>) {
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
                    println!(
                        "Adding {} by {} to the playlist data",
                        track.name.yellow(),
                        artists_string.cyan()
                    );
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

    #[tokio::test]
    #[ignore] // Requires .env credentials and network access. Run with `cargo test -- --ignored`
    async fn test_get_playlist() {
        dotenv().ok();
        let playlist_url = "https://open.spotify.com/playlist/6YYCPN91F4xI1Z17Hzn7ir".to_string();
        let mut playlist = SpotifyPlaylist::new(playlist_url).unwrap();
        playlist.get_playlist_info().await.unwrap();
        assert!(!playlist.tracks.is_empty());
        assert!(!playlist.name.is_empty());
    }

    #[tokio::test]
    #[ignore] // This test is interactive and requires manual user login. Run with `cargo test -- --ignored`
    async fn test_authorization_code_flow_with_pkce() {
        use rand::RngCore;
        use sha2::{Digest, Sha256};
        use tiny_http::{Response, Server};
        use webbrowser;

        // --- PKCE Step 1: Create a Code Verifier and Code Challenge ---
        // In a real app, you'd create these for each login attempt.

        // Create a random 32-byte string for the verifier.
        let mut verifier_bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut verifier_bytes);
        // Encode it using URL-safe base64. This is the "key".
        let code_verifier =
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&verifier_bytes);

        // Create the challenge by SHA256-hashing the verifier.
        let mut hasher = Sha256::new();
        hasher.update(code_verifier.as_bytes());
        let challenge_bytes = hasher.finalize();
        // The challenge is also encoded in URL-safe base64.
        let code_challenge =
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(challenge_bytes);

        // --- Standard Auth Flow Steps ---

        // 1. Load configuration. Note that SPOTIFY_CLIENT_SECRET is no longer needed.
        // In a real distributed app, the client_id would be hardcoded, not from a .env file.
        dotenv().ok();
        let client_id =
            env::var("SPOTIFY_CLIENT_ID").expect("SPOTIFY_CLIENT_ID must be set in .env");
        let redirect_uri = "http://localhost:8888/callback";
        let scopes = "playlist-read-private playlist-read-collaborative";

        // 2. Start a temporary local server to catch the redirect
        let server = Server::http("127.0.0.1:8888").unwrap();

        // 3. Construct the authorization URL, now including the PKCE parameters.
        let auth_url = format!(
            "https://accounts.spotify.com/authorize?response_type=code&client_id={}&scope={}&redirect_uri={}&code_challenge_method=S256&code_challenge={}",
            client_id,
            scopes.replace(' ', "%20"), // URL encode scopes
            redirect_uri,
            code_challenge
        );

        println!(
            "\n{}\n",
            "Please log in to Spotify in the browser window that just opened.".yellow()
        );
        if webbrowser::open(&auth_url).is_err() {
            println!(
                "Could not automatically open browser. Please copy/paste this URL:\n{}",
                auth_url.cyan()
            );
        }

        // 4. Wait for the user to log in and for Spotify to redirect back to our server
        let request = server
            .recv()
            .expect("Failed to receive request from browser");
        let full_url = format!("http://localhost:8888{}", request.url());
        let parsed_url = Url::parse(&full_url).unwrap();
        let auth_code = parsed_url
            .query_pairs()
            .find_map(|(key, value)| {
                if key == "code" {
                    Some(value.into_owned())
                } else {
                    None
                }
            })
            .expect("Could not find 'code' in callback URL");

        // Send a response to the browser so it doesn't hang
        let response = Response::from_string(
            "<h1>Authentication successful!</h1><p>You can close this browser tab now.</p>",
        );
        request.respond(response).unwrap();
        println!("\nAuthorization code received successfully!");

        // 5. Exchange the code for a token, sending the original code_verifier.
        // Note: No "Authorization: Basic" header is used.
        let client = reqwest::Client::new();
        let params = [
            ("grant_type", "authorization_code".to_string()),
            ("code", auth_code),
            ("redirect_uri", redirect_uri.to_string()),
            ("client_id", client_id),
            ("code_verifier", code_verifier),
        ];

        let token_response: serde_json::Value = client
            .post("https://accounts.spotify.com/api/token")
            .form(&params)
            .send()
            .await
            .expect("Failed to send token request")
            .json()
            .await
            .expect("Failed to parse token response");

        // 6. Print the credentials
        println!("\n--- User Credentials Obtained ---");
        let access_token = token_response["access_token"].as_str().unwrap_or("N/A");
        let refresh_token = token_response["refresh_token"].as_str().unwrap_or("N/A");

        println!("Access Token: {}", access_token.green());
        println!("Refresh Token: {}", refresh_token.yellow());
        println!(
            "\nFull response:\n{}",
            serde_json::to_string_pretty(&token_response).unwrap()
        );

        assert!(
            !access_token.is_empty() && access_token != "N/A",
            "Access token should not be empty"
        );
        assert!(
            !refresh_token.is_empty() && refresh_token != "N/A",
            "Refresh token should not be empty"
        );
    }
}
