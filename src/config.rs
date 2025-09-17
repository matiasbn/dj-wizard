/// `AppConfig` holds static configuration values for the application,
/// such as public client IDs for third-party services.
pub struct AppConfig;

impl AppConfig {
    /// The public client ID for the Spotify API, used for user authentication.
    pub const SPOTIFY_CLIENT_ID: &'static str = "a57ab1ceee1f4094b55924d3e228ae53";
    /// The public client ID for the Google Drive API, used for the backup feature.
    pub const GOOGLE_CLIENT_ID: &'static str =
        "84904078589-ads8pq2rols27kj3c3q4bv1aac5u8i7v.apps.googleusercontent.com";
    
    /// Gets the Google Client Secret from environment variable
    pub fn google_client_secret() -> Option<String> {
        std::env::var("GOOGLE_CLIENT_SECRET").ok()
    }
}
