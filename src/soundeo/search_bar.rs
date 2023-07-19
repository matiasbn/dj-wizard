use crate::soundeo::api::SoundeoAPI;
use crate::soundeo::{SoundeoError, SoundeoResult};
use crate::user::SoundeoUser;
use error_stack::{FutureExt, IntoReport, ResultExt};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::u32;
use strum_macros::Display;

fn deserialize_to_string<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let number: u32 = u32::deserialize(deserializer)?;
    Ok(format!("{number}"))
}

#[derive(Debug, Deserialize, Clone, strum_macros::Display)]
pub enum SoundeoSearchBar {
    Tracks,
    Artists,
    Releases,
    All,
}

impl SoundeoSearchBar {
    pub async fn search_term(
        &self,
        term: String,
        soundeo_user: &SoundeoUser,
    ) -> SoundeoResult<Vec<SoundeoSearchBarResult>> {
        let api_response = SoundeoAPI::GetSearchBarResult {
            term: term.replace(" ", "+").to_string(),
        }
        .get(soundeo_user)
        .await
        .change_context(SoundeoError)?;
        let result_vec: Vec<SoundeoSearchBarResult> = serde_json::from_str(&api_response)
            .into_report()
            .change_context(SoundeoError)?;
        return match self {
            SoundeoSearchBar::All => Ok(result_vec),
            _ => {
                let filtered_results: Vec<_> = result_vec
                    .into_iter()
                    .filter(|result| result.category == self.to_string())
                    .collect();
                Ok(filtered_results)
            }
        };
    }
}

#[derive(Debug, Deserialize, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SoundeoSearchBarResult {
    pub label: String,
    pub category: String,
    #[serde(deserialize_with = "deserialize_to_string")]
    pub value: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_get_search_bar_result() {
        let term = "Falling (Club Mix)".to_string();
        let mut soundeo_user = SoundeoUser::new().unwrap();
        soundeo_user.login_and_update_user_info().await.unwrap();
        let results = SoundeoSearchBar::All
            .search_term(term, &soundeo_user)
            .await
            .unwrap();
        println!("{:#?}", results);
    }

    #[tokio::test]
    async fn test_get_search_bar_result_tracks() {
        let track_id = "Falling (Club Mix)".to_string();
        let mut soundeo_user = SoundeoUser::new().unwrap();
        soundeo_user.login_and_update_user_info().await.unwrap();
        let results = SoundeoSearchBar::Tracks
            .search_term(track_id, &soundeo_user)
            .await
            .unwrap();
        println!("{:#?}", results);
    }
}
