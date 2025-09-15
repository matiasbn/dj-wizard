/// `AppConfig` holds static configuration values for the application,
/// such as public client IDs for third-party services.
pub struct AppConfig;

impl AppConfig {
    // Spotify Configuration
    /// The public client ID for the Spotify API, used for user authentication.
    pub const SPOTIFY_CLIENT_ID: &'static str = "a57ab1ceee1f4094b55924d3e228ae53";

    // Google OAuth Configuration
    /// The public client ID for Google OAuth Desktop application (no secret required).
    pub const GOOGLE_OAUTH_CLIENT_ID: &'static str =
        "84904078589-ads8pq2rols27kj3c3q4bv1aac5u8i7v.apps.googleusercontent.com";

    // Firebase Configuration
    /// Firebase API Key (public, safe to embed)
    pub const FIREBASE_API_KEY: &'static str = "AIzaSyCFVVzQ-yzrk7CK0b03SgQbyWJCoaBbq64";

    /// Firebase Auth Domain
    pub const FIREBASE_AUTH_DOMAIN: &'static str = "dj-wizard-firebase.firebaseapp.com";

    /// Firebase Project ID
    pub const FIREBASE_PROJECT_ID: &'static str = "dj-wizard-firebase";

    /// Firebase Storage Bucket
    pub const FIREBASE_STORAGE_BUCKET: &'static str = "dj-wizard-firebase.firebasestorage.app";

    /// Firebase Messaging Sender ID
    pub const FIREBASE_MESSAGING_SENDER_ID: &'static str = "295360168175";

    /// Firebase App ID
    pub const FIREBASE_APP_ID: &'static str = "1:295360168175:web:247c715fe7165376c2db1a";

    // OAuth Configuration
    /// OAuth callback port for local server
    pub const OAUTH_CALLBACK_PORT: u16 = 8080;

    /// OAuth callback timeout in seconds
    pub const OAUTH_CALLBACK_TIMEOUT_SECS: u64 = 120;
}
