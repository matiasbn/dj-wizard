use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use oauth2::basic::BasicClient;
use oauth2::reqwest::async_http_client;
use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, CsrfToken, PkceCodeChallenge, PkceCodeVerifier,
    RedirectUrl, RevocationUrl, Scope, TokenUrl,
};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use warp::Filter;

pub mod firebase_client;
pub mod google_auth;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthToken {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: chrono::DateTime<chrono::Utc>,
    pub user_email: String,
    pub user_id: String,
}

#[derive(Debug)]
pub struct AuthError(String);

impl AuthError {
    pub fn new(msg: &str) -> Self {
        Self(msg.to_string())
    }
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Authentication error: {}", self.0)
    }
}

impl std::error::Error for AuthError {}

impl From<firestore_db_and_auth::errors::FirebaseError> for AuthError {
    fn from(err: firestore_db_and_auth::errors::FirebaseError) -> Self {
        AuthError::new(&format!("Firebase error: {}", err))
    }
}

impl From<tonic::transport::Error> for AuthError {
    fn from(err: tonic::transport::Error) -> Self {
        AuthError::new(&format!("Transport error: {}", err))
    }
}

impl From<std::env::VarError> for AuthError {
    fn from(err: std::env::VarError) -> Self {
        AuthError::new(&format!("Environment variable error: {}", err))
    }
}

impl From<url::ParseError> for AuthError {
    fn from(err: url::ParseError) -> Self {
        AuthError::new(&format!("URL parse error: {}", err))
    }
}

impl From<reqwest::Error> for AuthError {
    fn from(err: reqwest::Error) -> Self {
        AuthError::new(&format!("HTTP request error: {}", err))
    }
}

impl From<serde_json::Error> for AuthError {
    fn from(err: serde_json::Error) -> Self {
        AuthError::new(&format!("JSON error: {}", err))
    }
}

impl From<std::io::Error> for AuthError {
    fn from(err: std::io::Error) -> Self {
        AuthError::new(&format!("IO error: {}", err))
    }
}

impl<T>
    From<
        oauth2::RequestTokenError<
            T,
            oauth2::StandardErrorResponse<oauth2::basic::BasicErrorResponseType>,
        >,
    > for AuthError
where
    T: std::fmt::Display + std::error::Error,
{
    fn from(
        err: oauth2::RequestTokenError<
            T,
            oauth2::StandardErrorResponse<oauth2::basic::BasicErrorResponseType>,
        >,
    ) -> Self {
        AuthError::new(&format!("OAuth token error: {}", err))
    }
}

impl From<Box<dyn std::error::Error + Send + Sync>> for AuthError {
    fn from(err: Box<dyn std::error::Error + Send + Sync>) -> Self {
        AuthError::new(&format!("Generic error: {}", err))
    }
}

pub type AuthResult<T> = error_stack::Result<T, AuthError>;
