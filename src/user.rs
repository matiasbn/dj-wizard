use std::fmt::Write;
use std::path::Path;
use std::{env, fmt, fs, string};

use colored::Colorize;
use error_stack::{FutureExt, IntoReport, Report, ResultExt};
use headless_chrome::protocol::cdp::Runtime::ConsoleAPICalledEventTypeOption::Dir;
use headless_chrome::Browser;
use lazy_regex::regex;
use reqwest::{Client, Response};
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::time::{sleep, Duration};

use crate::config::AppConfig;
use crate::{DjWizardCommands, Suggestion};

#[derive(Debug, Clone)]
pub struct SoundeoUserError;
impl fmt::Display for SoundeoUserError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SoundeoUser error")
    }
}
impl std::error::Error for SoundeoUserError {}

pub type SoundeoUserResult<T> = error_stack::Result<T, SoundeoUserError>;

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct IPFSConfig {
    pub api_key: String,
    pub api_key_secret: String,
    pub last_ipfs_hash: String,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct User {
    pub soundeo_user: String,
    pub soundeo_pass: String,
    pub download_path: String,
    #[serde(default)]
    pub ipfs: IPFSConfig,
    #[serde(default)]
    pub spotify_access_token: String,
    #[serde(default)]
    pub spotify_refresh_token: String,
    #[serde(default)]
    pub google_refresh_token: String,
}

impl User {
    pub fn new() -> Self {
        Self {
            soundeo_user: "".to_string(),
            soundeo_pass: "".to_string(),
            download_path: "".to_string(),
            ipfs: IPFSConfig {
                api_key: "".to_string(),
                api_key_secret: "".to_string(),
                last_ipfs_hash: "".to_string(),
            },
            spotify_access_token: "".to_string(),
            spotify_refresh_token: "".to_string(),
            google_refresh_token: "".to_string(),
        }
    }

    pub fn read_config_file(&mut self) -> SoundeoUserResult<()> {
        let soundeo_bot_config_path =
            User::get_config_file_path().attach_printable("Failed to get the config file path")?;
        if !Self::config_file_exists()? {
            return Err(Report::new(SoundeoUserError).attach_printable(format!(
                "Config file not found at: {}. Please create a config file first.",
                soundeo_bot_config_path
            )));
        }

        let config_content = fs::read_to_string(&soundeo_bot_config_path)
            .into_report()
            .attach_printable(format!(
                "Failed to read config file at {}",
                soundeo_bot_config_path
            ))
            .change_context(SoundeoUserError)?;
        let config: User = serde_json::from_str(&config_content)
            .into_report()
            .attach_printable("Failed to parse the config file. Ensure it is valid JSON.")
            .change_context(SoundeoUserError)?;

        if config.soundeo_pass.is_empty()
            || config.soundeo_user.is_empty()
            || config.download_path.is_empty()
        {
            return Err(Report::new(SoundeoUserError).attach_printable(format!(
                "Config file is incomplete. Please fill all fields in the config file at {}",
                soundeo_bot_config_path
            )));
        }
        self.clone_from(&config);
        Ok(())
    }

    pub fn create_new_config_file(&self) -> SoundeoUserResult<()> {
        let serialized = serde_json::to_string_pretty(self)
            .into_report()
            .attach_printable("Failed to serialize the user configuration to JSON")
            .change_context(SoundeoUserError)?;
        let config_path =
            Self::get_config_file_path().attach_printable("Failed to get the config file path")?;
        let folder_path = config_path.trim_end_matches("/config.json");
        if !Path::new(folder_path).exists() {
            fs::create_dir(folder_path)
                .into_report()
                .attach_printable(format!("Failed to create directory at {}", folder_path))
                .change_context(SoundeoUserError)?;
        }
        fs::write(config_path.clone(), serialized)
            .into_report()
            .attach_printable(format!("Failed to write config file at {}", config_path))
            .change_context(SoundeoUserError)?;
        Ok(())
    }

    pub fn get_config_file_path() -> SoundeoUserResult<String> {
        env::var("HOME")
            .into_report()
            .attach_printable("Failed to retrieve the HOME environment variable")
            .change_context(SoundeoUserError)
            .map(|home_path| format!("{}/.dj_wizard_config/config.json", home_path))
    }

    pub fn config_file_exists() -> SoundeoUserResult<bool> {
        let soundeo_bot_config_path =
            User::get_config_file_path().attach_printable("Failed to get the config file path")?;
        Ok(Path::new(&soundeo_bot_config_path).exists())
    }

    pub fn save_config_file(&self) -> SoundeoUserResult<()> {
        let save_log_string = serde_json::to_string_pretty(self)
            .into_report()
            .attach_printable("Failed to serialize the user configuration to JSON")
            .change_context(SoundeoUserError)?;
        let log_path =
            Self::get_config_file_path().attach_printable("Failed to get the config file path")?;
        fs::write(log_path.clone(), &save_log_string)
            .into_report()
            .attach_printable(format!("Failed to write config file at {}", log_path))
            .change_context(SoundeoUserError)?;
        Ok(())
    }

    pub async fn refresh_spotify_token(&mut self) -> SoundeoUserResult<()> {
        println!("Spotify access token expired. Attempting to refresh...");

        if self.spotify_refresh_token.is_empty() {
            return Err(Report::new(SoundeoUserError)
                .attach_printable("No refresh token available. Please log in again."));
        }

        let client_id = AppConfig::SPOTIFY_CLIENT_ID.to_string();
        let client = reqwest::Client::new();
        let params = [
            ("grant_type", "refresh_token".to_string()),
            ("refresh_token", self.spotify_refresh_token.clone()),
            ("client_id", client_id),
        ];

        let token_response: serde_json::Value = client
            .post("https://accounts.spotify.com/api/token")
            .form(&params)
            .send()
            .await
            .into_report()
            .change_context(SoundeoUserError)?
            .json()
            .await
            .into_report()
            .change_context(SoundeoUserError)?;

        if let Some(new_access_token) = token_response["access_token"].as_str() {
            self.spotify_access_token = new_access_token.to_string();

            if let Some(new_refresh_token) = token_response["refresh_token"].as_str() {
                self.spotify_refresh_token = new_refresh_token.to_string();
            }

            self.save_config_file()?;
            println!("{}", "Spotify token refreshed successfully.".green());
            Ok(())
        } else {
            Err(Report::new(SoundeoUserError).attach_printable(format!(
                "Failed to refresh Spotify token. Response: {:?}",
                token_response
            )))
        }
    }

    pub async fn refresh_google_token(&mut self) -> SoundeoUserResult<()> {
        println!("Google access token expired. Attempting to refresh...");

        if self.google_refresh_token.is_empty() {
            return Err(Report::new(SoundeoUserError)
                .attach_printable("No Google refresh token available. Please run 'dj-wizard auth' again."));
        }

        let client_id = AppConfig::GOOGLE_OAUTH_CLIENT_ID.to_string();
        let client_secret = std::env::var("GOOGLE_CLIENT_SECRET")
            .into_report()
            .change_context(SoundeoUserError)
            .attach_printable("GOOGLE_CLIENT_SECRET environment variable not set")?;
        
        let client = reqwest::Client::new();
        let params = [
            ("grant_type", "refresh_token".to_string()),
            ("refresh_token", self.google_refresh_token.clone()),
            ("client_id", client_id),
            ("client_secret", client_secret),
        ];

        let token_response: serde_json::Value = client
            .post("https://oauth2.googleapis.com/token")
            .form(&params)
            .send()
            .await
            .into_report()
            .change_context(SoundeoUserError)?
            .json()
            .await
            .into_report()
            .change_context(SoundeoUserError)?;

        if let Some(new_access_token) = token_response["access_token"].as_str() {
            // For Google tokens, we don't store the access token in User struct
            // Instead, we need to update the stored AuthToken file
            use crate::auth::{AuthToken, google_auth::GoogleAuth};
            
            // Load current token to preserve other fields
            let mut auth_token = match GoogleAuth::load_token().await {
                Ok(token) => token,
                Err(_) => {
                    // Create a new token structure if we can't load the existing one
                    AuthToken {
                        access_token: new_access_token.to_string(),
                        refresh_token: Some(self.google_refresh_token.clone()),
                        expires_at: chrono::Utc::now() + chrono::Duration::hours(1),
                        user_email: "".to_string(),
                        user_id: "".to_string(),
                    }
                }
            };

            // Update the access token
            auth_token.access_token = new_access_token.to_string();
            
            // Update refresh token if a new one is provided
            if let Some(new_refresh_token) = token_response["refresh_token"].as_str() {
                auth_token.refresh_token = Some(new_refresh_token.to_string());
                self.google_refresh_token = new_refresh_token.to_string();
            }

            // Update expiry time
            let expires_in = token_response["expires_in"]
                .as_u64()
                .unwrap_or(3600); // Default to 1 hour
            auth_token.expires_at = chrono::Utc::now() + chrono::Duration::seconds(expires_in as i64);

            // Save the updated token
            GoogleAuth::save_refreshed_token(&auth_token)
                .change_context(SoundeoUserError)?;
            
            // Also save this User config to persist the refresh token
            self.save_config_file()?;
            
            println!("{}", "Google token refreshed successfully.".green());
            Ok(())
        } else {
            Err(Report::new(SoundeoUserError).attach_printable(format!(
                "Failed to refresh Google token. Response: {:?}",
                token_response
            )))
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SoundeoUser {
    pub name: String,
    pub pass: String,
    pub download_path: String,
    pub cookie: String,
    pub snd: String,
    pub pk_id: String,
    pub pk_ses: String,
    pub bruid: String,
    pub snd_data: String,
    pub remaining_downloads: String,
    pub remaining_downloads_bonus: String,
    pub remaining_time_to_reset: String,
}

impl SoundeoUser {
    pub fn new() -> SoundeoUserResult<Self> {
        let mut config = User::new();
        config.read_config_file()?;
        Ok(Self {
            name: config.soundeo_user,
            pass: config.soundeo_pass,
            download_path: config.download_path,
            cookie: "".to_string(),
            snd: "".to_string(),
            pk_id: "".to_string(),
            pk_ses: "".to_string(),
            bruid: "".to_string(),
            snd_data: "".to_string(),
            remaining_downloads: "0".to_string(),
            remaining_downloads_bonus: "0".to_string(),
            remaining_time_to_reset: "".to_string(),
        })
    }

    pub fn get_remamining_downloads_string(&self) -> String {
        let string = if self.remaining_downloads_bonus == "0".to_string() {
            format!(
                "{} tracks before reaching the download limit",
                self.remaining_downloads.clone().cyan()
            )
        } else {
            format!(
                "{} (plus {} bonus) tracks before reaching the download limit",
                self.remaining_downloads.clone().cyan(),
                self.remaining_downloads_bonus.clone().cyan(),
            )
        };
        string
    }

    async fn get_cookie_from_browser(&mut self) -> SoundeoUserResult<()> {
        let browser = Browser::default()
            .ok()
            .ok_or_else(|| {
                Report::new(SoundeoUserError).attach_printable("Failed to initialize the browser")
            })
            .change_context(SoundeoUserError)?;

        let tab = browser
            .new_tab()
            .ok()
            .ok_or_else(|| {
                Report::new(SoundeoUserError).attach_printable("Failed to create a new browser tab")
            })
            .change_context(SoundeoUserError)?;
        tab.navigate_to("https://www.soundeo.com")
            .ok()
            .ok_or_else(|| {
                Report::new(SoundeoUserError)
                    .attach_printable("Failed to navigate to Soundeo website")
            })
            .change_context(SoundeoUserError)?;

        tab.wait_for_element("#userdata_el")
            .ok()
            .ok_or_else(|| {
                Report::new(SoundeoUserError)
                    .attach_printable("Failed to find the login element on the page")
            })
            .change_context(SoundeoUserError)?;
        let cookies = tab
            .get_cookies()
            .ok()
            .ok_or_else(|| {
                Report::new(SoundeoUserError)
                    .attach_printable("Failed to retrieve cookies from the browser")
            })
            .change_context(SoundeoUserError)?;
        for cookie in cookies {
            match cookie.name.as_str() {
                "_pk_id.1.5367" => {
                    self.pk_id = format!("_pk_id.1.5367={}", cookie.value);
                }
                "_pk_ses.1.5367" => {
                    self.pk_ses = format!("_pk_ses.1.5367={}", cookie.value);
                }
                "bruid" => {
                    self.bruid = format!("bruid={}", cookie.value);
                }
                "snd" => {
                    self.snd = format!("snd={}", cookie.value);
                }
                _ => {}
            }
        }
        self.cookie = format!(
            "{} ;{} ;{} ;{}",
            self.snd, self.pk_id, self.pk_ses, self.bruid
        );
        Ok(())
    }

    async fn get_login_response(&self) -> SoundeoUserResult<Response> {
        let client = Client::new();
        let body = format!(
            "_method=POST&data%5BUser%5D%5Blogin%5D={}&data%5BUser%5D%5Bpassword%5D={}&data%5Bremember%5D=1",
            self.name.replace("@", "%40"),
            self.pass
        );
        client
            .post("https://soundeo.com/account/logoreg")
            .body(body)
            .header("authority", "soundeo.com")
            .header("accept", "application/json, text/javascript, */*; q=0.01")
            .header("accept-language", "en-US,en;q=0.9")
            .header("content-type", "application/x-www-form-urlencoded; charset=UTF-8")
            .header("cookie", self.cookie.clone())
            .header("origin", "https://soundeo.com")
            .header("referer", "https://soundeo.com/")
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
            .await
            .into_report()
            .attach_printable("Failed to send login request to Soundeo")
            .change_context(SoundeoUserError)
    }

    async fn get_snd_data(&mut self, response: &Response) -> SoundeoUserResult<()> {
        let snd_data = response
            .headers()
            .get_all("set-cookie")
            .iter()
            .find(|header| header.to_str().unwrap_or_default().contains("snda[data]"))
            .ok_or_else(|| {
                Report::new(SoundeoUserError)
                    .attach_printable("Failed to find 'snda[data]' cookie in the response")
            })
            .and_then(|header| {
                header
                    .to_str()
                    .into_report()
                    .attach_printable("Failed to parse 'snda[data]' cookie as a string")
                    .change_context(SoundeoUserError)
            })?
            .to_string();
        self.snd_data = snd_data;
        Ok(())
    }

    pub async fn login_and_update_user_info(&mut self) -> SoundeoUserResult<()> {
        if self.cookie.is_empty() {
            println!("Logging in with {}", self.name.clone().green());
            self.get_cookie_from_browser()
                .await
                .attach_printable("Failed to retrieve cookies from the browser")?;
        }
        let mut logged_in = false;
        while !logged_in {
            let mut response = self.get_login_response().await;
            while response.is_err() {
                println!(
                    "{}",
                    colored::Colorize::red("Login response failed, retrying in 5 seconds")
                );
                sleep(Duration::from_secs(5)).await;
                response = self.get_login_response().await;
            }
            let response_unwrap = response.unwrap();
            let mut snd_data_result = self.get_snd_data(&response_unwrap).await;
            while snd_data_result.is_err() {
                println!(
                    "{}",
                    colored::Colorize::red(
                        "Failed to retrieve 'snda[data]', retrying in 5 seconds"
                    )
                );
                sleep(Duration::from_secs(5)).await;
                snd_data_result = self.get_snd_data(&response_unwrap).await;
            }
            let response_text = response_unwrap
                .text()
                .await
                .into_report()
                .attach_printable("Failed to retrieve response text")
                .change_context(SoundeoUserError)?;
            let json_resp: Value = serde_json::from_str(&response_text)
                .into_report()
                .attach_printable("Failed to parse response text as JSON")
                .change_context(SoundeoUserError)?;
            let header = json_resp["header"].clone().to_string();
            self.parse_remaining_downloads_and_wait_time(header)?;
            logged_in = true;
        }
        Ok(())
    }

    fn parse_remaining_downloads_and_wait_time(&mut self, header: String) -> SoundeoUserResult<()> {
        // Example
        // <span id='span-downloads'><span class=\"\" title=\"Main (will be reset in 2 hours 42 minutes 10 seconds)\">150</span></span>
        let header_downloads_regex = regex!(
            r#"<span id='span-downloads'>(.*?)<\/span>(?:\s*(?:\+\s*)?<span[^>]*>(.*?)<\/span>)?(<\/span>)?"#
        );

        let downloads_header = header_downloads_regex
            .find(&header)
            .ok_or(SoundeoUserError)
            .into_report()?
            .as_str()
            .to_string();

        let mut downloads_header_split = downloads_header
            .trim_start_matches(
                r#"<span id='span-downloads'><span class=\"\" title=\"Main (will be reset in ",
            )
            .trim_end_matches(r#"</span></span>"#,
            )
            .split(r#")\">"#);
        let remaining_time = downloads_header_split
            .next()
            .ok_or(SoundeoUserError)
            .into_report()?
            .trim_start_matches(
                r#"<span id='span-downloads'><span class=\"\" title=\"Main (will be reset in "#,
            )
            .to_string();
        let remaining_downloads_vec = self.get_remaining_downloads(header.clone())?;

        self.remaining_downloads = remaining_downloads_vec[0].clone();
        self.remaining_downloads_bonus = if remaining_downloads_vec.len() == 2 {
            remaining_downloads_vec[1].clone()
        } else {
            "0".to_string()
        };
        self.remaining_time_to_reset = remaining_time;
        Ok(())
    }

    fn get_remaining_downloads(&self, header: String) -> SoundeoUserResult<Vec<String>> {
        let document = Html::parse_document(&header);

        let selector = Selector::parse("#span-downloads span").unwrap();

        let numbers: Vec<String> = document
            .select(&selector)
            .map(|element| element.inner_html().as_str().to_string())
            .collect();

        Ok(numbers)
    }

    pub fn get_session_cookie(&self) -> SoundeoUserResult<String> {
        if self.snd_data.is_empty() {
            return Err(Report::new(SoundeoUserError).attach_printable("Session not initialized"));
        }
        Ok(format!(
            "{}; {}; {}; {}; {}",
            self.pk_id, self.pk_ses, self.snd, self.snd_data, self.bruid
        ))
    }
}

mod test {
    use super::*;

    #[test]
    fn test_get_value_with_bonus() {
        let html = r#"<li id="top-menu-downloads"><a href="/account/downloads"><i class="ico-downloads"></i><span id="span-downloads"><span class="active" title="Main (will be reset in 6 hours 57 minutes 9 seconds)">149</span> + <span class="" title="Bonus (can be used on any day with premium account)">300</span></span></a></li>"#;

        let document = Html::parse_document(html);

        let selector = Selector::parse("#span-downloads span").unwrap();

        let numbers: Vec<String> = document
            .select(&selector)
            .map(|element| element.inner_html().as_str().to_string())
            .collect();

        for number in numbers {
            println!("Number: {}", number);
        }
    }

    #[test]
    fn test_get_value_without_bonus() {
        let html = r#"<span id='span-downloads'><span class=\"\" title=\"Main (will be reset in 2 hours 42 minutes 10 seconds)\">150</span></span>"#;

        let document = Html::parse_document(html);

        let selector = Selector::parse("#span-downloads span").unwrap();

        let numbers: Vec<String> = document
            .select(&selector)
            .map(|element| element.inner_html().as_str().to_string())
            .collect();

        for number in numbers {
            println!("Number: {}", number);
        }
    }
}
