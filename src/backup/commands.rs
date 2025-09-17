use crate::user::User;
use colored::Colorize;
use error_stack::{IntoReport, Report, ResultExt};
use google_drive3::api::File;
use google_drive3::hyper::client::HttpConnector;
use google_drive3::hyper_rustls::HttpsConnector;
use google_drive3::{hyper, hyper_rustls, oauth2, DriveHub};
use std::default::Default;
use std::fs;
use std::io::Cursor;
use webbrowser;

use super::{BackupError, BackupResult};

pub struct BackupCommands;

impl BackupCommands {
    pub async fn execute() -> BackupResult<()> {
        let mut user_config = User::new();
        user_config.read_config_file().change_context(BackupError)?;

        // Create the hub with proper authentication
        println!("{}", "Connecting to Google Drive...".yellow());
        
        let refresh_token = if user_config.google_refresh_token.is_empty() {
            None
        } else {
            Some(user_config.google_refresh_token.as_str())
        };
        
        let (hub, new_refresh_token) = Self::create_authenticated_hub(refresh_token).await?;
        
        // Save new refresh token if we got one from OAuth flow
        if !new_refresh_token.is_empty() && new_refresh_token != user_config.google_refresh_token {
            user_config.google_refresh_token = new_refresh_token;
            user_config.save_config_file().change_context(BackupError)?;
            println!("{}", "Google credentials saved to user config.".green());
        }
        
        println!("{}", "Successfully connected to Google Drive.".green());

        Self::upload_log_to_drive(&hub, &user_config).await?;

        Ok(())
    }

    async fn create_authenticated_hub(
        refresh_token: Option<&str>,
    ) -> BackupResult<(DriveHub<HttpsConnector<HttpConnector>>, String)> {
        let client_id = crate::config::AppConfig::GOOGLE_CLIENT_ID;
        let client_secret =
            crate::config::AppConfig::google_client_secret().unwrap_or_else(|| String::new()); // Empty for PKCE

        let secret = oauth2::ApplicationSecret {
            client_id: client_id.to_string(),
            client_secret,
            token_uri: "https://oauth2.googleapis.com/token".to_string(),
            auth_uri: "https://accounts.google.com/o/oauth2/v2/auth".to_string(),
            redirect_uris: vec!["http://localhost:8889/callback".to_string()],
            project_id: None,
            client_email: None,
            client_x509_cert_url: None,
            auth_provider_x509_cert_url: None,
        };

        let auth = if let Some(existing_token) = refresh_token {
            // Use existing refresh token to get access token
            println!("Using existing Google credentials...");
            let token_string = Self::refresh_access_token_with_refresh_token(existing_token).await?;
            oauth2::AccessTokenAuthenticator::builder(token_string)
                .build()
                .await
                .into_report()
                .change_context(BackupError)?
        } else {
            // Do full OAuth flow
            println!("Starting Google OAuth flow...");
            oauth2::InstalledFlowAuthenticator::builder(
                secret,
                oauth2::InstalledFlowReturnMethod::HTTPRedirect,
            )
            .force_account_selection(true)
            .build()
            .await
            .into_report()
            .change_context(BackupError)?
        };

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

        // Try to get the refresh token for saving
        let refresh_token_value = if refresh_token.is_some() {
            // We used an existing token, return empty since we don't need to update
            String::new()
        } else {
            // We did OAuth flow, try to extract the refresh token
            // For now, we'll return empty and handle this properly later
            String::new()
        };

        Ok((hub, refresh_token_value))
    }

    async fn refresh_access_token_with_refresh_token(refresh_token: &str) -> BackupResult<String> {
        let client_id = crate::config::AppConfig::GOOGLE_CLIENT_ID;
        let client_secret = crate::config::AppConfig::google_client_secret()
            .unwrap_or_else(|| String::new());

        let client = reqwest::Client::new();
        let mut params = vec![
            ("client_id", client_id.to_string()),
            ("refresh_token", refresh_token.to_string()),
            ("grant_type", "refresh_token".to_string()),
        ];

        if !client_secret.is_empty() {
            params.push(("client_secret", client_secret));
        }

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

    async fn upload_log_to_drive(
        hub: &DriveHub<HttpsConnector<HttpConnector>>,
        user: &User,
    ) -> BackupResult<()> {
        println!("Starting backup to Google Drive...");

        let log_path =
            crate::log::DjWizardLog::get_log_path_from_config(user).change_context(BackupError)?;
        let log_content = fs::read(&log_path)
            .into_report()
            .attach_printable(format!("Failed to read log file at: {}", log_path))
            .change_context(BackupError)?;

        let backup_filename = "dj_wizard_log_backup.json";

        // 1. Check if the file already exists
        println!("Checking for existing backup file...");
        let result = hub
            .files()
            .list()
            .q(&format!("name = '{}' and trashed = false", backup_filename))
            .spaces("drive")
            .add_scope("https://www.googleapis.com/auth/drive")
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
                    .add_scope("https://www.googleapis.com/auth/drive")
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
                    .add_scope("https://www.googleapis.com/auth/drive")
                    .upload(Cursor::new(log_content), mime_type)
                    .await
                    .into_report()
                    .change_context(BackupError)?;
                println!("{}", "Backup created successfully!".green());
            }
        }

        Ok(())
    }
}
