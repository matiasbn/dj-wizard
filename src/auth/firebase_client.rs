// Firebase client implementation will go here
// For now, we'll focus on Google OAuth

use super::{AuthError, AuthResult, AuthToken};

pub struct FirebaseClient {
    // Will be implemented later
}

impl FirebaseClient {
    pub async fn new(_token: AuthToken) -> AuthResult<Self> {
        Ok(Self {})
    }
}