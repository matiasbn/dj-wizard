// crates/dj-wizard-core/src/user.rs
use std::path::{Path, PathBuf};
use std::{env, fs};

use headless_chrome::Browser; // WARN: Heavy/problematic dependency for core lib
use lazy_regex::regex;
use reqwest::{Client, Response};
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::time::{sleep, Duration};
use url::Url; // Needed for parsing check

use crate::error::{CoreError, Result};

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
    #[serde(default)] // Ensure ipfs is always present, even if missing in old files
    pub ipfs: IPFSConfig,
}

impl User {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn read_config_file(&mut self) -> Result<()> {
        let config_path = User::get_config_file_path()?;
        if !Self::config_file_exists()? {
            return Err(CoreError::Config(format!(
                "Config file not found at: {}",
                config_path
            )));
        }
        log::info!("Reading config file from: {}", config_path);
        let config_content = fs::read_to_string(&config_path)?;
        let config: User = serde_json::from_str(&config_content)?;

        if config.soundeo_user.is_empty()
            || config.soundeo_pass.is_empty()
            || config.download_path.is_empty()
        {
            return Err(CoreError::Config(format!(
                "Config file ({}) is missing required fields (user, pass, or download_path).",
                config_path
            )));
        }
        *self = config;
        Ok(())
    }

    pub fn save_config_file(&self) -> Result<()> {
        let config_path = Self::get_config_file_path()?;
        log::info!("Saving config file to: {}", config_path);
        let serialized = serde_json::to_string_pretty(self)?;

        let folder_path = Path::new(&config_path).parent().ok_or_else(|| {
            CoreError::Config(format!("Invalid config path structure: {}", config_path))
        })?;

        if !folder_path.exists() {
            fs::create_dir_all(folder_path)?;
        }

        fs::write(&config_path, serialized)?;
        Ok(())
    }

    pub fn get_config_file_path() -> Result<String> {
        let home_path = env::var("HOME")?; // Propagates VarError -> CoreError::Config
        Ok(format!("{}/.dj_wizard_config/config.json", home_path))
    }

    pub fn config_file_exists() -> Result<bool> {
        let config_path = User::get_config_file_path()?;
        Ok(Path::new(&config_path).exists())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SoundeoUser {
    pub name: String,
    pass: String, // Keep password private within the struct
    pub download_path: String,
    // Cookies are internal state, manage them carefully
    cookie: String,
    snd: String,
    pk_id: String,
    pk_ses: String,
    bruid: String,
    snd_data: String,
    // Status fields
    pub remaining_downloads: String,
    pub remaining_downloads_bonus: String,
    pub remaining_time_to_reset: String,
}

impl SoundeoUser {
    pub fn from_config(config: User) -> Self {
        Self {
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
        }
    }

    // Convenience constructor reading from default config file
    pub fn try_from_config_file() -> Result<Self> {
        let mut config = User::new();
        config.read_config_file()?;
        Ok(Self::from_config(config))
    }

    pub fn get_remamining_downloads_string(&self) -> String {
        if self.remaining_downloads_bonus == "0" {
            format!(
                "{} tracks before reaching the download limit",
                self.remaining_downloads
            )
        } else {
            format!(
                "{} (plus {} bonus) tracks before reaching the download limit",
                self.remaining_downloads, self.remaining_downloads_bonus,
            )
        }
    }

    // // NOTE: This function uses headless_chrome and is problematic for a core library.
    // // It might need to be feature-gated or replaced with another mechanism.
    // // Returning Result<()> indicates success/failure of getting *some* cookies.
    // #[cfg(feature = "browser_login")] // Example feature flag
    async fn get_initial_cookies_with_browser(&mut self) -> Result<()> {
        log::warn!("Attempting to use headless_chrome to get initial cookies.");
        let browser = Browser::default()
            .map_err(|e| CoreError::Browser(format!("Failed to launch browser: {:?}", e)))?;
        let tab = browser
            .new_tab()
            .map_err(|e| CoreError::Browser(format!("Failed to open new tab: {:?}", e)))?;
        tab.navigate_to("https://www.soundeo.com")
            .map_err(|e| CoreError::Browser(format!("Failed to navigate: {:?}", e)))?;
        tab.wait_for_element("#userdata_el") // Might be fragile
            .map_err(|e| {
                CoreError::Browser(format!("Failed to find #userdata_el element: {:?}", e))
            })?;
        let cookies = tab
            .get_cookies()
            .map_err(|e| CoreError::Browser(format!("Failed to get cookies: {:?}", e)))?;

        // Reset internal state before assignment
        self.pk_id = String::new();
        self.pk_ses = String::new();
        self.bruid = String::new();
        self.snd = String::new();
        self.cookie = String::new(); // Clear combined cookie too

        for cookie in cookies {
            match cookie.name.as_str() {
                "_pk_id.1.5367" => self.pk_id = format!("_pk_id.1.5367={}", cookie.value),
                "_pk_ses.1.5367" => self.pk_ses = format!("_pk_ses.1.5367={}", cookie.value),
                "bruid" => self.bruid = format!("bruid={}", cookie.value),
                "snd" => self.snd = format!("snd={}", cookie.value),
                _ => {}
            }
        }

        // Check if essential cookies were found
        if self.pk_id.is_empty()
            || self.pk_ses.is_empty()
            || self.bruid.is_empty()
            || self.snd.is_empty()
        {
            return Err(CoreError::Browser(
                "Failed to retrieve all essential initial cookies (_pk_id, _pk_ses, bruid, snd)"
                    .to_string(),
            ));
        }

        // Combine the essential *initial* cookies (snd_data comes later)
        self.cookie = format!(
            "{} ;{} ;{} ;{}",
            self.snd, self.pk_id, self.pk_ses, self.bruid
        );
        log::debug!("Initial cookies retrieved via browser.");
        Ok(())
    }

    async fn get_login_response(&self) -> Result<Response> {
        if self.cookie.is_empty() {
            // This check might be redundant if login_and_update calls get_initial_cookies first
            return Err(CoreError::Login(
                "Attempted login without initial cookies.".to_string(),
            ));
        }
        let client = Client::new();
        let encoded_user = urlencoding::encode(&self.name);
        let encoded_pass = urlencoding::encode(&self.pass); // Use the private pass field
        let body = format!(
            "_method=POST&data%5BUser%5D%5Blogin%5D={}&data%5BUser%5D%5Bpassword%5D={}&data%5Bremember%5D=1",
            encoded_user, encoded_pass
        );

        let response = client
            .post("https://soundeo.com/account/logoreg")
            .body(body)
            .header("authority", "soundeo.com")
            .header("accept", "application/json, text/javascript, */*; q=0.01")
            .header("accept-language", "en-US,en;q=0.9")
            .header(
                "content-type",
                "application/x-www-form-urlencoded; charset=UTF-8",
            )
            .header("cookie", self.cookie.clone()) // Send initial cookie
            .header("origin", "https://soundeo.com")
            .header("referer", "https://soundeo.com/")
            .header("user-agent", "Mozilla/5.0") // Generic UA
            .header("x-requested-with", "XMLHttpRequest")
            // Remove other potentially fingerprinting headers unless proven necessary
            .send()
            .await?;
        Ok(response)
    }

    // Updates internal cookie state based on login response headers
    fn update_session_state_from_response(&mut self, response: &Response) -> Result<()> {
        let mut found_snd_data = false;
        for header_value in response.headers().get_all("set-cookie").iter() {
            if let Ok(cookie_str) = header_value.to_str() {
                if let Some(cookie_part) = cookie_str.split(';').next() {
                    let trimmed_part = cookie_part.trim();
                    if trimmed_part.starts_with("snda[data]=") {
                        self.snd_data = trimmed_part.to_string();
                        found_snd_data = true;
                    } else if trimmed_part.starts_with("snd=") && !trimmed_part.contains("deleted")
                    {
                        self.snd = trimmed_part.to_string();
                    } // Only update snd_data and potentially snd from login response
                }
            } else {
                log::warn!("Received invalid non-UTF8 set-cookie header");
            }
        }

        if !found_snd_data {
            return Err(CoreError::Login(
                "Login failed: snda[data] cookie not received. Check credentials/account status."
                    .to_string(),
            ));
        }

        // Reconstruct the full session cookie string for later use
        self.cookie = self.get_session_cookie_string()?;
        log::debug!("Session cookie updated with snda[data].");
        Ok(())
    }

    // Public login function
    pub async fn login_and_update(&mut self) -> Result<()> {
        // // Attempt to get initial cookies via browser if enabled and needed
        // #[cfg(feature = "browser_login")]
        // if self.cookie.is_empty() {
        //     log::info!("No initial cookie found, attempting browser method...");
        //     self.get_initial_cookies_with_browser().await?;
        // }

        // For core lib, assume initial cookies must be set somehow *before* calling this,
        // or handle the lack of them gracefully. Let's error if initial cookies are missing.
        if self.pk_id.is_empty()
            || self.pk_ses.is_empty()
            || self.bruid.is_empty()
            || self.snd.is_empty()
        {
            return Err(CoreError::Login(
                    "Cannot attempt login: Initial essential cookies (pk_id, pk_ses, bruid, snd) are missing. \
                     These are typically obtained from visiting the site first.".to_string()
                ));
        }
        // Construct the cookie needed *for the login request itself*
        self.cookie = format!(
            "{} ;{} ;{} ;{}",
            self.snd, self.pk_id, self.pk_ses, self.bruid
        );

        log::info!("Attempting Soundeo login for user: {}", self.name);
        let mut last_error: Option<CoreError> = None;
        for attempt in 1..=3 {
            match self.get_login_response().await {
                Ok(response) => {
                    if !response.status().is_success() {
                        let status = response.status();
                        let body = response
                            .text()
                            .await
                            .unwrap_or_else(|_| String::from("Could not read response body"));
                        log::warn!(
                            "Login attempt {} failed with HTTP status {}: {}",
                            attempt,
                            status,
                            body
                        );
                        last_error = Some(CoreError::Login(format!("HTTP status {}", status)));
                        sleep(Duration::from_secs(2)).await;
                        continue;
                    }

                    // Try to update session cookie (gets snd_data)
                    if let Err(e) = self.update_session_state_from_response(&response) {
                        log::error!(
                            "Attempt {}: Failed processing login response cookies: {}",
                            attempt,
                            e
                        );
                        // If cookie update fails, it's likely bad credentials, don't retry.
                        return Err(e);
                    }

                    // Parse remaining downloads from body JSON
                    match response.text().await {
                        Ok(body_text) => {
                            match serde_json::from_str::<Value>(&body_text) {
                                Ok(json_resp) => {
                                    if let Some(header_html) = json_resp["header"].as_str() {
                                        if let Err(e) = self
                                            .parse_remaining_downloads_and_wait_time(header_html)
                                        {
                                            log::warn!("Login succeeded, but failed to parse remaining downloads: {}", e);
                                            // Login still considered successful, but state might be incomplete
                                        } else {
                                            log::info!(
                                                 "Login successful. Downloads: {} (+{}). Reset in: {}",
                                                 self.remaining_downloads, self.remaining_downloads_bonus, self.remaining_time_to_reset
                                             );
                                        }
                                        return Ok(()); // SUCCESS
                                    } else {
                                        log::warn!("Login succeeded, but 'header' missing in JSON response.");
                                        // Treat as success but with incomplete state? Or error? Let's accept for now.
                                        return Ok(());
                                    }
                                }
                                Err(e) => {
                                    log::error!("Attempt {}: Failed to parse login response JSON: {}. Body: {}", attempt, e, body_text);
                                    last_error = Some(CoreError::Json(e));
                                    sleep(Duration::from_secs(2)).await; // JSON error might be transient
                                    continue;
                                }
                            }
                        }
                        Err(e) => {
                            log::error!(
                                "Attempt {}: Failed to read login response body: {}",
                                attempt,
                                e
                            );
                            last_error = Some(CoreError::Network(e));
                            sleep(Duration::from_secs(2)).await;
                            continue;
                        }
                    }
                }
                Err(e) => {
                    log::error!(
                        "Attempt {}: Network error during login request: {}",
                        attempt,
                        e
                    );
                    last_error = Some(e);
                    sleep(Duration::from_secs(5)).await; // Longer wait for network issues
                }
            }
        }
        Err(last_error.unwrap_or_else(|| {
            CoreError::Login("Login failed after multiple attempts".to_string())
        }))
    }

    // Parses download counts and time from the HTML snippet in the login/API responses
    fn parse_remaining_downloads_and_wait_time(&mut self, header_html: &str) -> Result<()> {
        let document = Html::parse_fragment(header_html);
        let downloads_selector = Selector::parse("#span-downloads span").unwrap(); // Assume unwrap is ok for static selector
        let numbers: Vec<String> = document
            .select(&downloads_selector)
            .map(|el| el.inner_html().trim().to_string())
            .filter(|s| !s.is_empty() && s.chars().all(char::is_numeric))
            .collect();

        if numbers.is_empty() {
            log::warn!("Could not parse numeric download counts from header snippet.");
            self.remaining_downloads = "0".to_string();
            self.remaining_downloads_bonus = "0".to_string();
        } else {
            self.remaining_downloads = numbers.get(0).cloned().unwrap_or_else(|| "0".to_string());
            self.remaining_downloads_bonus =
                numbers.get(1).cloned().unwrap_or_else(|| "0".to_string());
        }

        let time_selector =
            Selector::parse("#span-downloads span[title*='will be reset in']").unwrap(); // Assume ok
        if let Some(element) = document.select(&time_selector).next() {
            if let Some(title) = element.value().attr("title") {
                if let Some(captures) = regex!(r"will be reset in (.*)\)").captures(title) {
                    self.remaining_time_to_reset = captures
                        .get(1)
                        .map_or("?".to_string(), |m| m.as_str().trim().to_string());
                } else {
                    log::warn!("Could not extract time from title attribute: {}", title);
                    self.remaining_time_to_reset = "?".to_string();
                }
            } else {
                log::warn!("Found time element but it's missing the title attribute.");
                self.remaining_time_to_reset = "?".to_string();
            }
        } else {
            log::warn!("Could not find element with reset time in header snippet.");
            self.remaining_time_to_reset = "?".to_string();
        }
        Ok(())
    }

    // Provides the full cookie string needed for subsequent API calls
    // Should only be called after a successful login/update
    pub fn get_session_cookie_string(&self) -> Result<String> {
        // Require all parts, especially snd_data which comes from login response
        if self.pk_id.is_empty()
            || self.pk_ses.is_empty()
            || self.snd.is_empty()
            || self.bruid.is_empty()
            || self.snd_data.is_empty()
        {
            return Err(CoreError::Login(
                "Session cookie requested but state is incomplete.".to_string(),
            ));
        }
        Ok(format!(
            "{}; {}; {}; {}; {}",
            self.pk_id, self.pk_ses, self.snd, self.snd_data, self.bruid
        ))
    }
}
