use crate::{SoundeoBotCommands, Suggestion};
use colored::Colorize;
use colorize::AnsiColor;
use error_stack::{FutureExt, IntoReport, Report, ResultExt};
use headless_chrome::protocol::cdp::Runtime::ConsoleAPICalledEventTypeOption::Dir;
use headless_chrome::Browser;
use lazy_regex::regex;
use reqwest::{Client, Response};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fmt::Write;
use std::path::Path;
use std::{env, fmt, fs, string};

#[derive(Debug)]
pub struct SoundeoUserError;
impl fmt::Display for SoundeoUserError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SoundeoUser error")
    }
}
impl std::error::Error for SoundeoUserError {}

pub type SoundeoUserResult<T> = error_stack::Result<T, SoundeoUserError>;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SoundeoUserConfig {
    pub user: String,
    pub pass: String,
    pub download_path: String,
}

impl SoundeoUserConfig {
    pub fn new() -> Self {
        Self {
            user: "".to_string(),
            pass: "".to_string(),
            download_path: "".to_string(),
        }
    }

    pub fn read_config_file(&mut self) -> SoundeoUserResult<()> {
        let soundeo_bot_config_path = SoundeoUserConfig::get_config_file_path()?;
        if !Self::config_file_exists()? {
            return Err(Report::new(SoundeoUserError).attach_printable(format!(
                "Config file not found at: {}",
                soundeo_bot_config_path
            )));
        }

        let config_content = fs::read_to_string(&soundeo_bot_config_path)
            .into_report()
            .change_context(SoundeoUserError)?;
        let config: SoundeoUserConfig = serde_json::from_str(&config_content)
            .into_report()
            .change_context(SoundeoUserError)?;

        if config.pass.is_empty() || config.user.is_empty() || config.download_path.is_empty() {
            return Err(Report::new(SoundeoUserError).attach_printable(format!(
                "Please fill all the fields of config.json file. Current file is at {}",
                soundeo_bot_config_path
            )));
        }
        self.user = config.user;
        self.pass = config.pass;
        self.download_path = config.download_path;
        Ok(())
    }

    pub fn create_new_config_file(&self) -> SoundeoUserResult<()> {
        let serialized = serde_json::to_string_pretty(self)
            .into_report()
            .change_context(SoundeoUserError)?;
        let config_path = Self::get_config_file_path()?;
        fs::write(config_path, serialized)
            .into_report()
            .change_context(SoundeoUserError)?;
        Ok(())
    }

    pub fn get_config_file_path() -> SoundeoUserResult<String> {
        let home_path = env::var("HOME")
            .into_report()
            .change_context(SoundeoUserError)?;
        let string_path = format!("{}/.soundeo_bot_config/config.json", home_path);
        Ok(string_path)
    }

    pub fn config_file_exists() -> SoundeoUserResult<bool> {
        let soundeo_bot_config_path = SoundeoUserConfig::get_config_file_path()?;
        Ok(Path::new(&soundeo_bot_config_path).exists())
    }
}

#[derive(Debug, Serialize, Deserialize)]
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
    pub remaining_time_to_reset: String,
}

impl SoundeoUser {
    pub fn new() -> SoundeoUserResult<Self> {
        let mut config = SoundeoUserConfig::new();
        config.read_config_file()?;
        Ok(Self {
            name: config.user,
            pass: config.pass,
            download_path: config.download_path,
            cookie: "".to_string(),
            snd: "".to_string(),
            pk_id: "".to_string(),
            pk_ses: "".to_string(),
            bruid: "".to_string(),
            snd_data: "".to_string(),
            remaining_downloads: "".to_string(),
            remaining_time_to_reset: "".to_string(),
        })
    }

    pub fn validate_remaining_downloads(&mut self) -> SoundeoUserResult<()> {
        if self.remaining_downloads == "0".to_string() {
            return Err(Report::new(SoundeoUserError)
                .attach_printable("No more downloads available")
                .attach(Suggestion(format!(
                    "Wait {} to start downloading again",
                    self.remaining_time_to_reset.clone().green()
                ))));
        }
        Ok(())
    }

    async fn get_cookie_from_browser(&mut self) -> SoundeoUserResult<()> {
        let browser = Browser::default()
            .ok()
            .ok_or(SoundeoUserError)
            .into_report()
            .change_context(SoundeoUserError)?;

        let tab = browser
            .new_tab()
            .ok()
            .ok_or(SoundeoUserError)
            .into_report()
            .change_context(SoundeoUserError)?;
        tab.navigate_to("https://www.soundeo.com")
            .ok()
            .ok_or(SoundeoUserError)
            .into_report()
            .change_context(SoundeoUserError)?;

        tab.wait_for_element("#userdata_el")
            .ok()
            .ok_or(SoundeoUserError)
            .into_report()
            .change_context(SoundeoUserError)?;
        let cookies = tab
            .get_cookies()
            .ok()
            .ok_or(SoundeoUserError)
            .into_report()
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

    pub async fn login_and_update_user_info(&mut self) -> SoundeoUserResult<()> {
        if self.cookie.is_empty() {
            println!("Login in with {}", self.name.clone().green());
            self.get_cookie_from_browser().await?;
        }
        let client = Client::new();
        let body = format!("_method=POST&data%5BUser%5D%5Blogin%5D={}&data%5BUser%5D%5Bpassword%5D={}&data%5Bremember%5D=1", self.name.replace("@", "%40"), self.pass);
        let response = client
            .post("https://soundeo.com/account/logoreg")
            .body(body)
            .header("authority","soundeo.com")
            .header("accept","application/json, text/javascript, */*; q=0.01")
            .header("accept-language","en-US,en;q=0.9")
            .header("content-type","application/x-www-form-urlencoded; charset=UTF-8")
            .header("cookie",self.cookie.clone())
            .header("origin","https://soundeo.com")
            .header("referer","https://soundeo.com/")
            .header("sec-ch-ua",r#"Not.A/Brand";v="8", "Chromium";v="114", "Brave";v="114"#)
            .header("sec-ch-ua-mobile","?0")
            .header("sec-ch-ua-platform","macOS")
            .header("sec-fetch-dest","empty")
            .header("sec-fetch-mode","cors")
            .header("sec-fetch-site","same-origin")
            .header("sec-gpc","1")
            .header("user-agent","Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/114.0.0.0 Safari/537.36")
            .header("x-requested-with","XMLHttpRequest")
            .send()
            .await.into_report().change_context(SoundeoUserError)?;
        let snd_data = response
            .headers()
            .get_all("set-cookie")
            .iter()
            .find(|header| header.to_str().unwrap().contains("snda[data]"))
            .ok_or(SoundeoUserError)
            .into_report()
            .attach_printable(format!("Incorrect user name and/or password"))
            .attach(Suggestion(format!(
                "Update the username and password by running {} ",
                SoundeoBotCommands::Login.cli_command().green()
            )))?
            .to_str()
            .into_report()
            .change_context(SoundeoUserError)?
            .to_string();
        self.snd_data = snd_data;
        let response_text = response
            .text()
            .await
            .into_report()
            .change_context(SoundeoUserError)?;
        let json_resp: Value = serde_json::from_str(&response_text)
            .into_report()
            .change_context(SoundeoUserError)?;
        let header = json_resp["header"].clone().to_string();
        self.parse_remaining_downloads_and_wait_time(header)?;
        Ok(())
    }

    fn parse_remaining_downloads_and_wait_time(&mut self, header: String) -> SoundeoUserResult<()> {
        // Example
        // <span id='span-downloads'><span class=\"\" title=\"Main (will be reset in 2 hours 42 minutes 10 seconds)\">0</span></span>
        let header_downloads_regex = regex!(
            r#"(<span id='span-downloads'><span class=\\"\\" title=\\")+[\w\(\) ]+(\\">)[0-9]+(<\/span><\/span>)"#
        );
        let downloads_header = header_downloads_regex
            .find(&header)
            .ok_or(SoundeoUserError)
            .into_report()?
            .as_str()
            .to_string();
        let mut downloads_header_split = downloads_header
            .trim_start_matches(
                r#"<span id='span-downloads'><span class=\"\" title=\"Main (will be reset in "#,
            )
            .trim_end_matches(r#"</span></span>"#)
            .split(r#")\">"#);
        let remaining_time = downloads_header_split
            .next()
            .ok_or(SoundeoUserError)
            .into_report()?
            .to_string();
        let remaining_downloads = downloads_header_split
            .next()
            .ok_or(SoundeoUserError)
            .into_report()?
            .to_string();

        self.remaining_downloads = remaining_downloads;
        self.remaining_time_to_reset = remaining_time;
        Ok(())
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
