//! X (Twitter) API client for fetching tweet data (used by Lark link previews).
//!
//! Uses X API v2 when `X_BEARER_TOKEN` is configured, falls back to the public
//! oEmbed API which requires no authentication.

use reqwest::Client;
use tracing::warn;

/// Minimal tweet data needed to build a Lark preview card.
pub struct TweetData {
    pub text: String,
    pub author_name: String,
    pub author_username: String,
    pub url: String,
}

/// Extracts the tweet/status ID from an X or Twitter URL.
///
/// Handles `https://x.com/user/status/1234567890` and
/// `https://twitter.com/user/status/1234567890`.
pub fn extract_tweet_id(url: &str) -> Option<&str> {
    let parts: Vec<&str> = url.split('/').collect();
    for (i, &part) in parts.iter().enumerate() {
        if part == "status"
            && let Some(&id) = parts.get(i + 1)
        {
            // Strip query params or fragments from the ID segment
            let id = id.split('?').next().unwrap_or(id);
            let id = id.split('#').next().unwrap_or(id);
            if !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()) {
                return Some(id);
            }
        }
    }
    None
}

/// Client for fetching tweet data.
pub struct XClient {
    bearer_token: Option<String>,
    http: Client,
}

impl XClient {
    pub fn new(bearer_token: Option<String>, http: Client) -> Self {
        Self { bearer_token, http }
    }

    /// Fetches tweet data. Tries X API v2 first (if bearer token is set),
    /// then falls back to oEmbed. Returns a minimal `TweetData` on total failure.
    pub async fn fetch(&self, tweet_id: &str, tweet_url: &str) -> TweetData {
        if let Some(token) = &self.bearer_token {
            match self.fetch_api_v2(tweet_id, token).await {
                Ok(data) => return data,
                Err(e) => warn!("X API v2 failed, falling back to oEmbed: {e}"),
            }
        }
        match self.fetch_oembed(tweet_url).await {
            Ok(data) => data,
            Err(e) => {
                warn!("X oEmbed also failed: {e}");
                TweetData {
                    text: String::new(),
                    author_name: String::new(),
                    author_username: String::new(),
                    url: tweet_url.to_string(),
                }
            }
        }
    }

    async fn fetch_api_v2(&self, tweet_id: &str, bearer_token: &str) -> Result<TweetData, String> {
        let url = format!(
            "https://api.twitter.com/2/tweets/{}?expansions=author_id&user.fields=name,username",
            tweet_id
        );
        let resp = self
            .http
            .get(&url)
            .bearer_auth(bearer_token)
            .send()
            .await
            .map_err(|e| format!("request error: {e}"))?;

        if !resp.status().is_success() {
            return Err(format!("X API returned {}", resp.status()));
        }

        let body: serde_json::Value = resp.json().await.map_err(|e| format!("parse error: {e}"))?;

        let text = body["data"]["text"].as_str().unwrap_or("").to_string();
        let author_name = body["includes"]["users"][0]["name"]
            .as_str()
            .unwrap_or("")
            .to_string();
        let author_username = body["includes"]["users"][0]["username"]
            .as_str()
            .unwrap_or("")
            .to_string();

        Ok(TweetData {
            text,
            author_name,
            author_username,
            url: format!("https://x.com/i/web/status/{tweet_id}"),
        })
    }

    async fn fetch_oembed(&self, tweet_url: &str) -> Result<TweetData, String> {
        let url = format!(
            "https://publish.twitter.com/oembed?url={}&omit_script=true",
            tweet_url
        );
        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("request error: {e}"))?;

        if !resp.status().is_success() {
            return Err(format!("oEmbed returned {}", resp.status()));
        }

        let body: serde_json::Value = resp.json().await.map_err(|e| format!("parse error: {e}"))?;

        let author_name = body["author_name"].as_str().unwrap_or("").to_string();

        Ok(TweetData {
            text: String::new(),
            author_name,
            author_username: String::new(),
            url: tweet_url.to_string(),
        })
    }
}
