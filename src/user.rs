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

    pub async fn check_remaining_downloads(&mut self) -> SoundeoUserResult<(u32, u32)> {
        let response = self.get_login_response().await?;
        let response_text = response
            .text()
            .await
            .into_report()
            .attach_printable("Failed to retrieve response text for downloads check")
            .change_context(SoundeoUserError)?;
        let json_resp: Value = serde_json::from_str(&response_text)
            .into_report()
            .attach_printable("Failed to parse response text as JSON for downloads check")
            .change_context(SoundeoUserError)?;
        let header = json_resp["header"]
            .as_str()
            .ok_or(SoundeoUserError)
            .into_report()?
            .to_string();
        
        // Parse downloads using existing method
        let remaining_downloads_vec = self.get_remaining_downloads(header.clone())?;
        
        let main_downloads = remaining_downloads_vec[0].parse::<u32>()
            .into_report()
            .attach_printable("Failed to parse main downloads as number")
            .change_context(SoundeoUserError)?;
        
        let bonus_downloads = if remaining_downloads_vec.len() == 2 {
            remaining_downloads_vec[1].parse::<u32>()
                .into_report()
                .attach_printable("Failed to parse bonus downloads as number")
                .change_context(SoundeoUserError)?
        } else {
            0
        };
        
        Ok((main_downloads, bonus_downloads))
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
            let header = json_resp["header"]
                .as_str()
                .ok_or(SoundeoUserError)
                .into_report()?
                .to_string();
            self.parse_remaining_downloads_and_wait_time(header)?;
            logged_in = true;
        }
        Ok(())
    }

    fn parse_remaining_downloads_and_wait_time(&mut self, header: String) -> SoundeoUserResult<()> {
        // Examples:
        // Without bonus: <span id='span-downloads'><span class="" title="Main (will be reset in 18 hours 54 minutes 50 seconds)">148</span></span>
        // With bonus: <span id='span-downloads'><span class="" title="Main (will be reset in 6 hours 57 minutes 9 seconds)">149</span> + <span class="" title="Bonus">300</span></span>
        
        // Extract the remaining time from the title attribute
        let time_regex = regex!(r#"title="[^"]*will be reset in ([^")]+)"#);
        let remaining_time = time_regex
            .captures(&header)
            .and_then(|caps| caps.get(1))
            .map(|m| m.as_str().to_string())
            .unwrap_or_else(|| "unknown".to_string());

        // Extract downloads using the existing method
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

    #[test]
    fn test_get_remamining_downloads_string() {
        match SoundeoUser::new() {
            Ok(user) => {
                println!("User loaded successfully");
                println!("remaining_downloads: {}", user.remaining_downloads);
                println!("remaining_downloads_bonus: {}", user.remaining_downloads_bonus);
                let result = user.get_remamining_downloads_string();
                println!("Function result: {}", result);
            }
            Err(e) => {
                println!("Error loading user config: {:?}", e);
            }
        }
    }

    #[test]
    fn test_check_remaining_downloads() {
        // Test the new check_remaining_downloads method
        println!("=== TESTING CHECK REMAINING DOWNLOADS ===");
        
        match SoundeoUser::new() {
            Ok(mut user) => {
                println!("User loaded: {}", user.name);
                
                let rt = tokio::runtime::Runtime::new().unwrap();
                
                match rt.block_on(user.check_remaining_downloads()) {
                    Ok((main_downloads, bonus_downloads)) => {
                        println!("✅ CHECK SUCCESSFUL!");
                        println!("🔢 Main downloads: {}", main_downloads);
                        println!("🎁 Bonus downloads: {}", bonus_downloads);
                        println!("💯 Total downloads: {}", main_downloads + bonus_downloads);
                        
                        // Basic validations
                        println!("✅ Downloads check completed!");
                    }
                    Err(e) => {
                        println!("❌ CHECK FAILED: {:?}", e);
                        println!("⚠️  This test requires valid credentials");
                        return;
                    }
                }
            }
            Err(e) => {
                println!("❌ Error loading user configuration: {:?}", e);
                println!("⚠️  This test requires a valid configuration file");
                return;
            }
        }
    }

    #[test]
    fn test_real_login_and_parse() {
        // Test that actually logs into Soundeo and gets real values
        println!("=== REAL SOUNDEO LOGIN TEST ===");
        
        match SoundeoUser::new() {
            Ok(mut user) => {
                println!("User loaded: {}", user.name);
                
                // Use tokio runtime for async login
                let rt = tokio::runtime::Runtime::new().unwrap();
                
                match rt.block_on(user.login_and_update_user_info()) {
                    Ok(()) => {
                        println!("✅ LOGIN SUCCESSFUL!");
                        println!("🔢 Remaining downloads: {}", user.remaining_downloads);
                        println!("🎁 Bonus downloads: {}", user.remaining_downloads_bonus);
                        println!("⏰ Reset time: {}", user.remaining_time_to_reset);
                        println!("📝 Generated string: {}", user.get_remamining_downloads_string());
                        
                        // Basic validations
                        assert!(!user.remaining_downloads.is_empty(), "Downloads should not be empty");
                        assert!(!user.remaining_time_to_reset.is_empty(), "Reset time should not be empty");
                        
                        // Verify they are valid numbers
                        let downloads_num: Result<u32, _> = user.remaining_downloads.parse();
                        let bonus_num: Result<u32, _> = user.remaining_downloads_bonus.parse();
                        
                        assert!(downloads_num.is_ok(), "Downloads should be a valid number: {}", user.remaining_downloads);
                        assert!(bonus_num.is_ok(), "Bonus should be a valid number: {}", user.remaining_downloads_bonus);
                        
                        println!("✅ All values are valid!");
                    }
                    Err(e) => {
                        println!("❌ LOGIN FAILED: {:?}", e);
                        println!("⚠️  This test requires valid credentials in configuration");
                        // Don't fail the test if there are no valid credentials
                        return;
                    }
                }
            }
            Err(e) => {
                println!("❌ Error loading user configuration: {:?}", e);
                println!("⚠️  This test requires a valid configuration file");
                // Don't fail the test if there's no configuration
                return;
            }
        }
    }

    #[test]
    fn test_scrapper_with_real_webpage() {
        // HTML real extraído de webpage_test.html
        let real_webpage_html = r#"<ul class="top-menu">
							<li id="top-menu-downloads"><a href="https://soundeo.com/account/downloads"><i class="ico-downloads"></i><span id="span-downloads"><span class="" title="Main (will be reset in 18 hours 54 minutes 50 seconds)">148</span></span></a></li>		<li id="top-menu-votes"><a href="https://soundeo.com/account/votes"><i class="ico-votes"></i><span id="span-votes"><span class="active" title="Main (will be reset in 18 hours 54 minutes 50 seconds)">30</span> + <span class="" title="Bonus (can be used on any day with premium account)">60</span></span></a></li>		<li id="top-menu-favorites"><a href="https://soundeo.com/account/favorites"><i class="ico-favorites"></i><span id="span-favorites">48</span></a></li>				<li id="top-menu-account"><a href="https://soundeo.com/account"><i class="ico-account"></i><span title="1 day 5 hours 53 minutes 46 seconds">1 day</span></a></li>		<li id="top-menu-logout"><a href="https://soundeo.com/account/logout"><i class="ico-logout"></i></a></li>	</ul>"#;
        
        match SoundeoUser::new() {
            Ok(mut user) => {
                println!("=== ANÁLISIS DEL SCRAPPER CON PÁGINA REAL ===");
                println!("HTML real: {}", real_webpage_html);
                
                // Test del selector actual
                println!("\n--- Testing selector actual: '#span-downloads span' ---");
                let downloads_vec = user.get_remaining_downloads(real_webpage_html.to_string()).unwrap();
                println!("Elementos encontrados por selector downloads: {:?}", downloads_vec);
                
                // Test con el HTML completo para parse_remaining_downloads_and_wait_time
                println!("\n--- Testing parse_remaining_downloads_and_wait_time ---");
                match user.parse_remaining_downloads_and_wait_time(real_webpage_html.to_string()) {
                    Ok(()) => {
                        println!("✅ Parse exitoso!");
                        println!("remaining_downloads: {}", user.remaining_downloads);
                        println!("remaining_downloads_bonus: {}", user.remaining_downloads_bonus);
                        println!("remaining_time_to_reset: {}", user.remaining_time_to_reset);
                        let result = user.get_remamining_downloads_string();
                        println!("String generado: {}", result);
                    }
                    Err(e) => {
                        println!("❌ Parse falló: {:?}", e);
                    }
                }
                
                // Test del selector para votes (que SÍ tiene bonus)
                println!("\n--- Testing selector votes como comparación: '#span-votes span' ---");
                let document = scraper::Html::parse_document(&real_webpage_html);
                let votes_selector = scraper::Selector::parse("#span-votes span").unwrap();
                let votes_elements: Vec<String> = document
                    .select(&votes_selector)
                    .map(|element| element.inner_html().as_str().to_string())
                    .collect();
                println!("Elementos encontrados por selector votes: {:?}", votes_elements);
            }
            Err(e) => {
                println!("Could not load user config: {:?}", e);
            }
        }
    }

    #[test]
    fn test_parse_remaining_downloads_and_wait_time() {
        // HTML real actual de la página de Soundeo (sin bonus)
        let test_html_no_bonus = r#"<li id="top-menu-downloads"><a href="https://soundeo.com/account/downloads"><i class="ico-downloads"></i><span id="span-downloads"><span class="" title="Main (will be reset in 18 hours 54 minutes 50 seconds)">148</span></span></a></li>"#;
        
        // HTML con bonus
        let test_html_with_bonus = r#"<li id="top-menu-downloads"><a href="/account/downloads"><i class="ico-downloads"></i><span id="span-downloads"><span class="" title="Main (will be reset in 6 hours 57 minutes 9 seconds)">149</span> + <span class="" title="Bonus (can be used on any day with premium account)">300</span></span></a></li>"#;
        
        // Test JSON string format (como viene del server)
        let test_json_string = r#""<li id=\"top-menu-downloads\"><a href=\"/account/downloads\"><i class=\"ico-downloads\"></i><span id=\"span-downloads\"><span class=\"\" title=\"Main (will be reset in 18 hours 54 minutes 50 seconds)\">148</span></span></a></li>""#;
        
        match SoundeoUser::new() {
            Ok(mut user) => {
                println!("=== Testing WITHOUT bonus ===");
                println!("HTML: {}", test_html_no_bonus);
                match user.parse_remaining_downloads_and_wait_time(test_html_no_bonus.to_string()) {
                    Ok(()) => {
                        println!("Parse successful!");
                        println!("remaining_downloads: {}", user.remaining_downloads);
                        println!("remaining_downloads_bonus: {}", user.remaining_downloads_bonus);
                        println!("remaining_time_to_reset: {}", user.remaining_time_to_reset);
                        let result = user.get_remamining_downloads_string();
                        println!("Generated string: {}", result);
                    }
                    Err(e) => {
                        println!("Parse failed: {:?}", e);
                    }
                }
                
                println!("\n=== Testing WITH bonus ===");
                println!("HTML: {}", test_html_with_bonus);
                match user.parse_remaining_downloads_and_wait_time(test_html_with_bonus.to_string()) {
                    Ok(()) => {
                        println!("Parse successful!");
                        println!("remaining_downloads: {}", user.remaining_downloads);
                        println!("remaining_downloads_bonus: {}", user.remaining_downloads_bonus);
                        println!("remaining_time_to_reset: {}", user.remaining_time_to_reset);
                        let result = user.get_remamining_downloads_string();
                        println!("Generated string: {}", result);
                    }
                    Err(e) => {
                        println!("Parse failed: {:?}", e);
                    }
                }
                
                println!("\n=== Testing JSON string format (como viene del server) ===");
                println!("JSON string: {}", test_json_string);
                // Simular el comportamiento correcto con .as_str()
                let json_val: serde_json::Value = serde_json::from_str(test_json_string).unwrap();
                let header_from_json = json_val.as_str().unwrap_or("").to_string();
                println!("Header after .as_str(): {}", header_from_json);
                match user.parse_remaining_downloads_and_wait_time(header_from_json) {
                    Ok(()) => {
                        println!("Parse successful!");
                        println!("remaining_downloads: {}", user.remaining_downloads);
                        println!("remaining_downloads_bonus: {}", user.remaining_downloads_bonus);
                        println!("remaining_time_to_reset: {}", user.remaining_time_to_reset);
                        let result = user.get_remamining_downloads_string();
                        println!("Generated string: {}", result);
                    }
                    Err(e) => {
                        println!("Parse failed: {:?}", e);
                    }
                }
            }
            Err(e) => {
                println!("Could not load user config: {:?}", e);
            }
        }
    }
}
