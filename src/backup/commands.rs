use crate::user::User;
use crate::{DjWizardCommands, Suggestion};
use base64::{engine::general_purpose, Engine as _};
use colored::Colorize;
use error_stack::{IntoReport, Report, ResultExt};
use google_drive3::api::{File, Scope};
use google_drive3::hyper::client::HttpConnector;
use google_drive3::hyper_rustls::HttpsConnector;
use google_drive3::{hyper, hyper_rustls, oauth2, DriveHub};
use rand::RngCore;
use sha2::{Digest, Sha256};
use std::default::Default;
use std::fs;
use std::io::Cursor;
use tiny_http::{Response, Server};
use url::Url;
use webbrowser;

use super::{BackupError, BackupResult};

pub struct BackupCommands;

impl BackupCommands {
    pub async fn execute() -> BackupResult<()> {
        let mut user_config = User::new();
        user_config.read_config_file().change_context(BackupError)?;

        if user_config.google_refresh_token.is_empty() {
            println!(
                "{}",
                "Google account not linked. Starting login process...".yellow()
            );
            let refresh_token = Self::perform_google_login()
                .await
                .attach_printable("Failed to log in with Google")?;
            user_config.google_refresh_token = refresh_token;
            user_config.save_config_file().change_context(BackupError)?;
            println!(
                "{}",
                "Google account successfully linked and credentials saved.".green()
            );
        }

        Self::upload_log_to_drive(&user_config).await?;

        Ok(())
    }

    async fn refresh_google_token(user: &User) -> BackupResult<String> {
        // For refreshing a token obtained via PKCE, the client_secret is not needed.
        let client_id = "YOUR_GOOGLE_CLIENT_ID"; // TODO: Replace with your Google Client ID

        let client = reqwest::Client::new();
        let params = [
            ("client_id", client_id),
            ("refresh_token", &user.google_refresh_token),
            ("grant_type", "refresh_token"),
        ];

        let token_response: serde_json::Value = client
            .post("https://oauth2.googleapis.com/token")
            .form(&params)
            .send()
            .await
            .into_report()
            .change_context(BackupError)?
            .json()
            .await
            .into_report()
            .change_context(BackupError)?;

        token_response["access_token"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| {
                Report::new(BackupError).attach_printable(format!(
                    "Failed to get access_token from refresh response: {:?}",
                    token_response
                ))
            })
    }

    async fn get_hub(user: &User) -> BackupResult<DriveHub<HttpsConnector<HttpConnector>>> {
        let access_token = Self::refresh_google_token(user).await?;
        let auth = oauth2::AccessTokenAuthenticator::builder(access_token)
            .build()
            .await
            .into_report()
            .change_context(BackupError)?;

        let http_client = hyper::Client::builder().build(
            hyper_rustls::HttpsConnectorBuilder::new()
                .with_native_roots()
                .into_report()
                .change_context(BackupError)?
                .https_or_http()
                .enable_http1()
                .build(),
        );

        let hub = DriveHub::new(http_client, auth);
        Ok(hub)
    }

    async fn upload_log_to_drive(user: &User) -> BackupResult<()> {
        println!("Starting backup to Google Drive...");
        let hub = Self::get_hub(user).await?;

        let log_path =
            crate::log::DjWizardLog::get_log_path_from_config(user).change_context(BackupError)?;
        let log_content = fs::read(&log_path)
            .into_report()
            .attach_printable(format!("Failed to read log file at: {}", log_path))
            .change_context(BackupError)?;

        let backup_filename = "dj_wizard_log_backup.json";

        // 1. Check if the file already exists
        let result = hub
            .files()
            .list()
            .q(&format!("name = '{}' and trashed = false", backup_filename))
            .spaces("drive")
            .doit()
            .await
            .into_report()
            .change_context(BackupError)?;

        let file_metadata = File {
            name: Some(backup_filename.to_string()),
            ..Default::default()
        };

        let mime_type = "application/json".parse().unwrap();

        if let Some(files) = result.1.files {
            if let Some(existing_file) = files.first() {
                // File exists, update it
                let file_id = existing_file.id.as_ref().unwrap();
                println!("Found existing backup file. Updating...");
                hub.files()
                    .update(file_metadata, file_id)
                    .upload(Cursor::new(log_content), mime_type)
                    .await
                    .into_report()
                    .change_context(BackupError)?;
                println!("{}", "Backup updated successfully!".green());
            } else {
                // File does not exist, create it
                println!("No existing backup file found. Creating a new one...");
                hub.files()
                    .create(file_metadata)
                    .upload(Cursor::new(log_content), mime_type)
                    .await
                    .into_report()
                    .change_context(BackupError)?;
                println!("{}", "Backup created successfully!".green());
            }
        }

        Ok(())
    }

    async fn perform_google_login() -> BackupResult<String> {
        // --- PKCE Step 1: Create a Code Verifier and Code Challenge ---
        let mut verifier_bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut verifier_bytes);
        let code_verifier =
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&verifier_bytes);

        let mut hasher = Sha256::new();
        hasher.update(code_verifier.as_bytes());
        let challenge_bytes = hasher.finalize();
        let code_challenge =
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(challenge_bytes);

        // --- Standard Auth Flow Steps ---
        let client_id = "48225261965-t4hfd2n53gtl9qvj9sqh03vtobv5dboh.apps.googleusercontent.com";
        let redirect_uri = "http://localhost:8889/callback";
        let scopes = "https://www.googleapis.com/auth/drive.file";

        // 2. Start a temporary local server to catch the redirect
        let server = Server::http("127.0.0.1:8889").unwrap();

        // 3. Construct the authorization URL with PKCE params
        let auth_url = format!(
            "https://accounts.google.com/o/oauth2/v2/auth?scope={}&response_type=code&redirect_uri={}&client_id={}&code_challenge={}&code_challenge_method=S256",
            scopes, redirect_uri, client_id, code_challenge
        );

        println!(
            "\n{}\n",
            "Please log in to Google in the browser window that just opened.".yellow()
        );
        if webbrowser::open(&auth_url).is_err() {
            println!(
                "Could not automatically open browser. Please copy/paste this URL:\n{}",
                auth_url.cyan()
            );
        }

        // 4. Wait for the user to log in and for Google to redirect back to our server
        let request = server.recv().into_report().change_context(BackupError)?;
        let full_url = format!("http://localhost:8889{}", request.url());
        let parsed_url = Url::parse(&full_url)
            .into_report()
            .change_context(BackupError)?;
        let auth_code = parsed_url
            .query_pairs()
            .find_map(|(key, value)| {
                if key == "code" {
                    Some(value.into_owned())
                } else {
                    None
                }
            })
            .ok_or_else(|| {
                Report::new(BackupError).attach_printable("Could not find 'code' in callback URL")
            })?;

        let response = Response::from_string(
            "<h1>Authentication successful!</h1><p>You can close this browser tab now.</p>",
        );
        request
            .respond(response)
            .into_report()
            .change_context(BackupError)?;
        println!("\nAuthorization code received successfully!");

        // 5. Exchange the code for a token, sending the original code_verifier
        let client = reqwest::Client::new();
        let params = [
            ("client_id", client_id.to_string()),
            ("code", auth_code),
            ("code_verifier", code_verifier),
            ("grant_type", "authorization_code".to_string()),
            ("redirect_uri", redirect_uri.to_string()),
        ];

        let token_response: serde_json::Value = client
            .post("https://oauth2.googleapis.com/token")
            .form(&params)
            .send()
            .await
            .into_report()
            .change_context(BackupError)?
            .json()
            .await
            .into_report()
            .change_context(BackupError)?;

        // 6. Extract and return the refresh token
        token_response["refresh_token"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| {
                Report::new(BackupError).attach_printable(format!(
                    "Google did not provide a refresh token. Please try logging in again.",
                ))
            })
    }
}
