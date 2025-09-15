use std::time::Duration;

use colored::Colorize;
use error_stack::{IntoReport, ResultExt};
use oauth2::basic::BasicClient;
use oauth2::reqwest::async_http_client;
use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, PkceCodeChallenge,
    PkceCodeVerifier, RedirectUrl, Scope, TokenResponse, TokenUrl,
};
use serde::{Deserialize, Serialize};

use super::{AuthError, AuthResult, AuthToken};
use crate::config::AppConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoogleUserInfo {
    pub id: String,
    pub email: String,
    pub name: String,
    pub picture: Option<String>,
}

pub struct GoogleAuth;

impl GoogleAuth {
    pub fn new() -> Self {
        Self
    }

    pub async fn login(&self) -> AuthResult<AuthToken> {
        println!("{}", "🔐 Initiating Google authentication...".cyan());

        // Create OAuth2 client - reading client secret from environment variable
        // TODO: Fix this properly - Google shouldn't require client secret for Desktop apps with PKCE
        let client_secret = std::env::var("GOOGLE_CLIENT_SECRET")
            .into_report()
            .attach_printable("❌ Error: GOOGLE_CLIENT_SECRET environment variable not set")
            .attach_printable("Please set it with:")
            .attach_printable("echo 'export GOOGLE_CLIENT_SECRET=\"your_secret_here\"' >> ~/.zshrc && source ~/.zshrc")
            .change_context(AuthError::new("Request error"))?;

        let client = BasicClient::new(
            ClientId::new(AppConfig::GOOGLE_OAUTH_CLIENT_ID.to_string()),
            Some(ClientSecret::new(client_secret)),
            AuthUrl::new("https://accounts.google.com/o/oauth2/v2/auth".to_string())
                .into_report()
                .change_context(AuthError::new("Auth URL parse error"))?,
            Some(
                TokenUrl::new("https://oauth2.googleapis.com/token".to_string())
                    .into_report()
                    .change_context(AuthError::new("Token URL parse error"))?,
            ),
        )
        .set_redirect_uri(
            RedirectUrl::new("http://localhost:8080/callback".to_string())
                .into_report()
                .change_context(AuthError::new("Redirect URL parse error"))?,
        );

        // Generate PKCE challenge (this replaces the client secret)
        let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();

        // Generate authorization URL
        let (auth_url, csrf_token) = client
            .authorize_url(CsrfToken::new_random)
            .add_scope(Scope::new("openid".to_string()))
            .add_scope(Scope::new("email".to_string()))
            .add_scope(Scope::new("profile".to_string()))
            .add_scope(Scope::new("https://www.googleapis.com/auth/datastore".to_string()))
            .set_pkce_challenge(pkce_challenge)
            .url();

        println!("Opening browser for authentication...");
        println!(
            "If browser doesn't open, visit: {}",
            auth_url.to_string().blue()
        );

        // Open browser
        if webbrowser::open(auth_url.as_str()).is_err() {
            println!("{}", "Failed to open browser automatically".yellow());
        }

        // Start local server to receive callback
        let (code, returned_state) = Self::start_callback_server().await?;

        // Verify CSRF token (state parameter)
        if !returned_state.is_empty() && returned_state != *csrf_token.secret() {
            println!(
                "Warning: State mismatch - expected: {}, got: {}",
                csrf_token.secret(),
                returned_state
            );
        }

        // Exchange code for token using PKCE verifier
        println!("Exchanging authorization code for access token...");
        let token_response = client
            .exchange_code(AuthorizationCode::new(code))
            .set_pkce_verifier(pkce_verifier)
            .request_async(async_http_client)
            .await
            .into_report()
            .attach_printable("Token exchange failed")
            .change_context(AuthError::new("Token exchange error"))?;

        println!("✅ Token exchange successful!");

        // Get user info
        let user_info = self
            .get_user_info(token_response.access_token().secret())
            .await?;

        println!(
            "✅ Successfully authenticated as: {}",
            user_info.email.green()
        );

        // Create auth token
        let auth_token = AuthToken {
            access_token: token_response.access_token().secret().to_string(),
            refresh_token: token_response
                .refresh_token()
                .map(|t| t.secret().to_string()),
            expires_at: chrono::Utc::now()
                + chrono::Duration::seconds(
                    token_response
                        .expires_in()
                        .map(|d| d.as_secs() as i64)
                        .unwrap_or(3600),
                ),
            user_email: user_info.email,
            user_id: user_info.id,
        };

        // Save token locally
        self.save_token(&auth_token)?;

        Ok(auth_token)
    }

    async fn start_callback_server() -> AuthResult<(String, String)> {
        use tiny_http::{Header, Response, Server};

        let server = Server::http("localhost:8080")
            .map_err(|e| AuthError::new(&format!("Failed to start server: {}", e)))?;

        println!("Waiting for authentication callback on http://localhost:8080/callback");

        let timeout = Duration::from_secs(AppConfig::OAUTH_CALLBACK_TIMEOUT_SECS);
        let start = std::time::Instant::now();

        loop {
            if start.elapsed() > timeout {
                println!("{}", "Authentication timeout!".red());
                return Err(AuthError::new("Authentication timeout")).into_report();
            }

            // Try to receive request with timeout
            if let Ok(Some(request)) = server.recv_timeout(Duration::from_millis(100)) {
                let url = request.url();
                println!("Received request: {}", url);

                if url.starts_with("/callback") {
                    // Parse query parameters
                    if let Some(query) = url.split('?').nth(1) {
                        println!("Parsing query: {}", query);
                        let mut code = None;
                        let mut state = None;

                        for param in query.split('&') {
                            let parts: Vec<&str> = param.split('=').collect();
                            if parts.len() == 2 {
                                match parts[0] {
                                    "code" => {
                                        code = Some(
                                            urlencoding::decode(parts[1])
                                                .unwrap_or_default()
                                                .to_string(),
                                        );
                                        println!("Found code: {}", code.as_ref().unwrap());
                                    }
                                    "state" => {
                                        state = Some(
                                            urlencoding::decode(parts[1])
                                                .unwrap_or_default()
                                                .to_string(),
                                        );
                                        println!("Found state: {}", state.as_ref().unwrap());
                                    }
                                    _ => {}
                                }
                            }
                        }

                        if let Some(auth_code) = code {
                            println!("Sending success response and returning auth code");
                            // Send success response
                            let html = r#"
                                <html>
                                <head>
                                    <title>Authentication Successful</title>
                                    <style>
                                        body {
                                            font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
                                            display: flex;
                                            justify-content: center;
                                            align-items: center;
                                            height: 100vh;
                                            margin: 0;
                                            background: linear-gradient(135deg, #667eea 0%, #764ba2 100%);
                                        }
                                        .container {
                                            text-align: center;
                                            background: white;
                                            padding: 40px;
                                            border-radius: 10px;
                                            box-shadow: 0 10px 40px rgba(0,0,0,0.1);
                                        }
                                        h1 { color: #2d3748; }
                                        p { color: #718096; }
                                        .checkmark {
                                            font-size: 64px;
                                            color: #48bb78;
                                        }
                                    </style>
                                </head>
                                <body>
                                    <div class="container">
                                        <div class="checkmark">✓</div>
                                        <h1>Authentication Successful!</h1>
                                        <p>You can close this window and return to DJ Wizard.</p>
                                    </div>
                                </body>
                                </html>
                            "#;

                            let response = Response::from_string(html).with_header(
                                Header::from_bytes(&b"Content-Type"[..], &b"text/html"[..])
                                    .unwrap(),
                            );
                            let _ = request.respond(response);

                            return Ok((auth_code, state.unwrap_or_default()));
                        }
                    }
                }

                // Send 404 for other requests
                let response = Response::from_string("Not Found").with_status_code(404);
                let _ = request.respond(response);
            } else {
                // Timeout or no request, continue loop
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }
    }

    async fn get_user_info(&self, access_token: &str) -> AuthResult<GoogleUserInfo> {
        let client = reqwest::Client::new();
        let response = client
            .get("https://www.googleapis.com/oauth2/v2/userinfo")
            .bearer_auth(access_token)
            .send()
            .await
            .into_report()
            .change_context(AuthError::new("Request error"))?;

        let user_info: GoogleUserInfo = response
            .json()
            .await
            .into_report()
            .change_context(AuthError::new("Request error"))?;

        Ok(user_info)
    }

    fn save_token(&self, token: &AuthToken) -> AuthResult<()> {
        // Save to local file in user's home directory
        let home_dir = dirs::home_dir()
            .ok_or(AuthError::new("Could not find home directory"))
            .into_report()?;
        let token_path = home_dir.join(".dj-wizard").join("auth_token.json");

        // Create directory if it doesn't exist
        std::fs::create_dir_all(token_path.parent().unwrap())
            .into_report()
            .change_context(AuthError::new("Request error"))?;

        // Save token
        let token_json = serde_json::to_string_pretty(token)
            .into_report()
            .change_context(AuthError::new("Request error"))?;
        std::fs::write(token_path, token_json)
            .into_report()
            .change_context(AuthError::new("Request error"))?;

        Ok(())
    }

    pub fn load_token() -> AuthResult<AuthToken> {
        let home_dir = dirs::home_dir()
            .ok_or(AuthError::new("Could not find home directory"))
            .into_report()?;
        let token_path = home_dir.join(".dj-wizard").join("auth_token.json");

        let token_json = std::fs::read_to_string(token_path)
            .into_report()
            .change_context(AuthError::new("Request error"))?;
        let token: AuthToken = serde_json::from_str(&token_json)
            .into_report()
            .change_context(AuthError::new("Request error"))?;

        // Check if token is expired
        if token.expires_at < chrono::Utc::now() {
            println!("{}", "Token expired, please login again".yellow());
            return Err(AuthError::new("Token expired")).into_report();
        }

        Ok(token)
    }
}
