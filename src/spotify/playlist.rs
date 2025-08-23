use std::collections::HashMap;
use std::time::Duration;

use crate::dialoguer::Dialoguer;
use crate::log::DjWizardLog;
use colored::Colorize;
// use colorize::AnsiColor; // Remove or comment out this import to avoid ambiguity
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
                new_window: Some(true),
                background: None,
                for_tab: None,
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

        println!("The playlist name is {}", name.clone());
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
                title.clone(),
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
    use std::env;

    use colored::Colorize;
    use dotenvy::dotenv;
    use serde::Deserialize;

    use super::*;

    #[tokio::test]
    async fn test_get_playlist() {
        let playlist_url = "https://open.spotify.com/playlist/6YYCPN91F4xI1Z17Hzn7ir".to_string();
        let mut playlist = SpotifyPlaylist::new(playlist_url).unwrap();
        playlist.get_playlist_info().await.unwrap();
    }

    #[tokio::test]
    #[ignore] // Se ignora para no fallar en CI si no hay credenciales. Ejecutar con: cargo test -- --ignored
    async fn test_get_playlist_from_api() {
        // --- PASO 1: Cargar las credenciales desde .env ---
        // Carga las variables del archivo .env en el directorio raíz del proyecto.
        dotenv().ok();

        let client_id = env::var("SPOTIFY_CLIENT_ID").expect(
            "La variable de entorno SPOTIFY_CLIENT_ID no está definida. Asegúrate de tener un archivo .env con las credenciales.",
        );
        let client_secret = env::var("SPOTIFY_CLIENT_SECRET").expect(
            "La variable de entorno SPOTIFY_CLIENT_SECRET no está definida. Asegúrate de tener un archivo .env con las credenciales.",
        );
        let playlist_id = "0HUmClXaFGvEIi1rPvfqxg"; // Playlist de ejemplo de tu archivo

        // Verifica que las credenciales no sean los placeholders
        if client_id == "TU_CLIENT_ID" || client_secret == "TU_CLIENT_SECRET" {
            println!("\n{}", colored::Colorize::yellow("ATENCIÓN:"));
            println!("Por favor, reemplaza los valores en tu archivo .env con tus credenciales reales de la API de Spotify.");
            println!("Puedes obtenerlas en: https://developer.spotify.com/dashboard\n");
            // La prueba se saltará si las credenciales son las de placeholder
            return;
        }

        // --- PASO 2: Obtener el Token de Acceso (Client Credentials Flow) ---
        #[derive(Deserialize, Debug)]
        struct TokenResponse {
            access_token: String,
        }

        let client = reqwest::Client::new();
        let auth_string = format!("{}:{}", client_id, client_secret);
        let encoded_auth = base64::encode(auth_string);

        let token_response = client
            .post("https://accounts.spotify.com/api/token")
            .header("Authorization", format!("Basic {}", encoded_auth))
            .form(&[("grant_type", "client_credentials")])
            .send()
            .await
            .expect("Fallo al solicitar el token de acceso")
            .json::<TokenResponse>()
            .await
            .expect("Fallo al parsear la respuesta del token");

        let access_token = token_response.access_token;
        println!("Token de acceso obtenido con éxito!");

        // --- PASO 3: Obtener las canciones de la playlist ---
        let tracks_url = format!(
            "https://api.spotify.com/v1/playlists/{}/tracks",
            playlist_id
        );

        let response_text = client
            .get(&tracks_url)
            .bearer_auth(&access_token)
            .send()
            .await
            .expect("Fallo al obtener las canciones de la playlist")
            .text()
            .await
            .expect("Fallo al leer la respuesta de las canciones");

        // --- PASO 4: Imprimir los resultados ---
        let v: serde_json::Value =
            serde_json::from_str(&response_text).expect("Fallo al parsear el JSON");
        println!("\n--- Canciones en la Playlist '{}' ---", playlist_id);
        if let Some(items) = v["items"].as_array() {
            for item in items {
                if let Some(track) = item.get("track") {
                    if !track.is_null() {
                        let track_name = track["name"].as_str().unwrap_or("N/A");
                        let artists: Vec<String> = track["artists"]
                            .as_array()
                            .unwrap_or(&vec![])
                            .iter()
                            .map(|a| a["name"].as_str().unwrap_or("N/A").to_string())
                            .collect();
                        println!("- {} por {}", track_name, artists.join(", ").cyan());
                    }
                }
            }
        }
    }
}
