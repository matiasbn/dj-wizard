/// `AppConfig` holds static configuration values for the application,
/// such as public client IDs for third-party services.
pub struct AppConfig;

impl AppConfig {
    /// The public client ID for the Spotify API, used for user authentication.
    pub const SPOTIFY_CLIENT_ID: &'static str = "a57ab1ceee1f4094b55924d3e228ae53";
    /// The public client ID for the Google Drive API, used for the backup feature.
    pub const GOOGLE_CLIENT_ID: &'static str =
        "48225261965-t4hfd2n53gtl9qvj9sqh03vtobv5dboh.apps.googleusercontent.com";
}
