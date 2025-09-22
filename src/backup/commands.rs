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

use super::{BackupError, BackupResult};

pub struct BackupCommands;

impl BackupCommands {
    pub async fn execute() -> BackupResult<()> {
        let mut user_config = User::new();
        user_config.read_config_file().change_context(BackupError)?;

        // Create the hub with proper authentication
        println!("{}", "Connecting to Google Drive...".cyan());
        let hub = Self::create_authenticated_hub().await?;
        println!("{}", "Successfully connected to Google Drive.".green());

        match Self::upload_log_to_drive(&hub, &user_config)
            .await
            .change_context(BackupError)
        {
            Ok(()) => {}
            Err(ref e)
                if e.to_string().contains("SERVICE_DISABLED")
                    || e.to_string().contains("accessNotConfigured") =>
            {
                println!(
                    "{}",
                    "❌ Google Drive API is not enabled in your Google Cloud project.".yellow()
                );
                println!("{}", "Please enable it by visiting:".yellow());
                println!("{}", "https://console.developers.google.com/apis/api/drive.googleapis.com/overview?project=84904078589".cyan());
                println!(
                    "{}",
                    "After enabling, wait a few minutes and try again.".yellow()
                );
                return Err(
                    Report::new(BackupError).attach_printable("Google Drive API not enabled")
                );
            }
            Err(e) => return Err(e),
        }

        Ok(())
    }

    async fn create_authenticated_hub() -> BackupResult<DriveHub<HttpsConnector<HttpConnector>>> {
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

        // Define the path for the token cache file within the app's config directory.
        let token_cache_path = User::get_config_file_path()
            .change_context(BackupError)?
            .replace("config.json", "google_token_cache.json");

        let auth = oauth2::InstalledFlowAuthenticator::builder(
            secret,
            oauth2::InstalledFlowReturnMethod::HTTPRedirect,
        )
        .persist_tokens_to_disk(&token_cache_path)
        .force_account_selection(true)
        .build()
        .await
        .into_report()
        .change_context(BackupError)?;

        // The first time .token() is called, it will either use the cached token,
        // refresh it, or start the full web-based authentication flow.
        let scopes = &["https://www.googleapis.com/auth/drive.file"];
        match auth.token(scopes).await {
            Err(e) => {
                return Err(Report::new(BackupError)
                    .attach_printable(format!("Failed to get Google authentication token: {}", e)))
            }
            Ok(_) => println!("{}", "Google authentication successful.".green()),
        }

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

    async fn upload_log_to_drive(
        hub: &DriveHub<HttpsConnector<HttpConnector>>,
        user: &User,
    ) -> Result<(), Report<BackupError>> {
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
            .add_scope("https://www.googleapis.com/auth/drive.file")
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
                    .add_scope("https://www.googleapis.com/auth/drive.file")
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
                    .add_scope("https://www.googleapis.com/auth/drive.file")
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
