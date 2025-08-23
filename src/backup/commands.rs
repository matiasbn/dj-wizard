use crate::user::User;
use crate::{DjWizardCommands, Suggestion};
use colored::Colorize;
use error_stack::{IntoReport, Report, ResultExt};
use google_drive3::api::{File, Scope};
use google_drive3::hyper::client::HttpConnector;
use google_drive3::hyper_rustls::HttpsConnector;
use google_drive3::{hyper, hyper_rustls, oauth2, DriveHub};
use std::default::Default;
use std::fs;

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

    async fn get_hub(user: &User) -> BackupResult<DriveHub<HttpsConnector<HttpConnector>>> {
        let secret = oauth2::ApplicationSecret {
            client_id: "a57ab1ceee1f4094b55924d3e228ae53".to_string(),
            client_secret: "".to_string(), // Not needed for desktop app flow
            token_uri: "https://oauth2.googleapis.com/token".to_string(),
            auth_uri: "https://accounts.google.com/o/oauth2/auth".to_string(),
            redirect_uris: vec!["http://localhost:8889/callback".to_string()],
            ..Default::default()
        };

        let mut token = oauth2::Token::from(serde_json::json!({
            "refresh_token": user.google_refresh_token
        }));
        token.set_client_secret(secret.client_id.clone());

        let auth = oauth2::InstalledFlowAuthenticator::builder(
            secret,
            oauth2::InstalledFlowReturnMethod::HTTPRedirect,
        )
        .build()
        .await
        .into_report()
        .change_context(BackupError)?;

        let hub = DriveHub::new(
            hyper::Client::builder().build(
                hyper_rustls::HttpsConnectorBuilder::new()
                    .with_native_roots()
                    .https_or_http()
                    .enable_http1()
                    .build(),
            ),
            auth,
        );
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
        let secret = oauth2::ApplicationSecret {
            client_id: "a57ab1ceee1f4094b55924d3e228ae53".to_string(),
            client_secret: "".to_string(), // Not needed for desktop app flow
            token_uri: "https://oauth2.googleapis.com/token".to_string(),
            auth_uri: "https://accounts.google.com/o/oauth2/auth".to_string(),
            redirect_uris: vec!["http://localhost:8889/callback".to_string()],
            ..Default::default()
        };

        let auth = oauth2::InstalledFlowAuthenticator::builder(
            secret,
            oauth2::InstalledFlowReturnMethod::HTTPRedirect,
        )
        .build()
        .await
        .into_report()
        .change_context(BackupError)?;

        let scopes = &[Scope::DriveFile.as_ref()];
        let token = auth
            .token(scopes)
            .await
            .into_report()
            .change_context(BackupError)?;

        token.refresh_token.ok_or_else(|| {
            Report::new(BackupError).attach_printable(
                "Google did not provide a refresh token. Please try logging in again.",
            )
        })
    }
}
