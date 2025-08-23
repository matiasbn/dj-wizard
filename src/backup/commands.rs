use crate::user::User;
use crate::{DjWizardCommands, Suggestion};
use async_trait::async_trait;
use colored::Colorize;
use error_stack::{IntoReport, Report, ResultExt};
use google_drive3::api::{File, Scope};
use google_drive3::hyper::client::HttpConnector;
use google_drive3::hyper_rustls::HttpsConnector;
use google_drive3::oauth2::storage::{TokenInfo, TokenStorage};
use google_drive3::{hyper, hyper_rustls, oauth2, DriveHub};
use std::default::Default;
use std::fs;
use std::io::Cursor;
use std::sync::{Arc, Mutex as StdMutex};

use super::{BackupError, BackupResult};

#[derive(Clone, Default)]
struct EphemeralTokenStorage {
    token: Arc<StdMutex<Option<TokenInfo>>>,
}

#[async_trait]
impl TokenStorage for EphemeralTokenStorage {
    async fn set(&self, _scopes: &[&str], token: TokenInfo) -> anyhow::Result<()> {
        *self.token.lock().unwrap() = Some(token);
        Ok(())
    }

    async fn get(&self, _scopes: &[&str]) -> Option<TokenInfo> {
        self.token.lock().unwrap().clone()
    }
}

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
        // Use the same placeholders as the login function.
        // These credentials are required to refresh the token.
        let client_id = "YOUR_GOOGLE_CLIENT_ID";
        let client_secret = "YOUR_GOOGLE_CLIENT_SECRET";

        let client = reqwest::Client::new();
        let params = [
            ("client_id", client_id),
            ("client_secret", client_secret),
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

        let hub = DriveHub::new(
            hyper::Client::builder().build(
                hyper_rustls::HttpsConnectorBuilder::new()
                    .with_native_roots()
                    .into_report()
                    .change_context(BackupError)?
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
        // TODO: Replace these placeholder values with your own credentials from the Google Cloud Console.
        let client_id = "YOUR_GOOGLE_CLIENT_ID";
        let client_secret = "YOUR_GOOGLE_CLIENT_SECRET";

        let secret = oauth2::ApplicationSecret {
            client_id: client_id.to_string(),
            client_secret: client_secret.to_string(),
            token_uri: "https://oauth2.googleapis.com/token".to_string(),
            auth_uri: "https://accounts.google.com/o/oauth2/auth".to_string(),
            redirect_uris: vec!["http://localhost:8889/callback".to_string()],
            ..Default::default()
        };

        let storage = EphemeralTokenStorage::default();

        let auth = oauth2::InstalledFlowAuthenticator::builder(
            secret,
            oauth2::InstalledFlowReturnMethod::HTTPRedirect,
        )
        .with_storage(Box::new(storage.clone()))
        .build()
        .await
        .into_report()
        .change_context(BackupError)?;

        let scopes = &[Scope::File.as_ref()];
        auth.token(scopes)
            .await
            .into_report()
            .change_context(BackupError)?;

        let token_info = storage.get(scopes).await.ok_or_else(|| {
            Report::new(BackupError).attach_printable("Failed to retrieve token after login flow.")
        })?;

        token_info.refresh_token.ok_or_else(|| {
            Report::new(BackupError).attach_printable(
                "Google did not provide a refresh token. Please try logging in again.",
            )
        })
    }
}
