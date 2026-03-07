//! X (Twitter) API client for fetching tweet data (used by Lark link previews).
//!
//! Fetch priority:
//! 1. fxtwitter (`api.fxtwitter.com`) — no auth, rich data including metrics
//! 2. X API v2 — requires `X_BEARER_TOKEN`, adds nothing over fxtwitter for our use case
//! 3. oEmbed — no auth, minimal data (author name + HTML-extracted text)

use reqwest::Client;
use tracing::warn;

/// Minimal tweet data needed to build a Lark preview card.
pub struct TweetData {
    pub text: String,
    pub author_name: String,
    pub author_username: String,
    pub url: String,
    pub like_count: Option<u64>,
    pub retweet_count: Option<u64>,
    pub reply_count: Option<u64>,
}

/// Extracts `(username, tweet_id)` from an X or Twitter URL.
///
/// Handles `https://x.com/user/status/1234567890` and
/// `https://twitter.com/user/status/1234567890`.
pub fn extract_tweet_info(url: &str) -> Option<(&str, &str)> {
    let parts: Vec<&str> = url.split('/').collect();
    for (i, &part) in parts.iter().enumerate() {
        if part == "status" {
            let id = parts.get(i + 1).copied()?;
            let id = id.split('?').next().unwrap_or(id);
            let id = id.split('#').next().unwrap_or(id);
            if !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()) {
                let username = parts.get(i.wrapping_sub(1)).copied().unwrap_or("");
                return Some((username, id));
            }
        }
    }
    None
}

/// Extracts only the tweet ID (kept for compatibility).
pub fn extract_tweet_id(url: &str) -> Option<&str> {
    extract_tweet_info(url).map(|(_, id)| id)
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

    /// Fetches tweet data. Tries fxtwitter first, then X API v2, then oEmbed.
    pub async fn fetch(&self, tweet_id: &str, tweet_url: &str) -> TweetData {
        // 1. fxtwitter — best option, no auth needed
        let username = extract_tweet_info(tweet_url).map(|(u, _)| u).unwrap_or("");
        if !username.is_empty() {
            match self.fetch_fxtwitter(username, tweet_id).await {
                Ok(data) => return data,
                Err(e) => warn!("fxtwitter failed, trying next source: {e}"),
            }
        }

        // 2. X API v2 (optional bearer token)
        if let Some(token) = &self.bearer_token {
            match self.fetch_api_v2(tweet_id, token).await {
                Ok(data) => return data,
                Err(e) => warn!("X API v2 failed, falling back to oEmbed: {e}"),
            }
        }

        // 3. oEmbed fallback
        match self.fetch_oembed(tweet_url).await {
            Ok(data) => data,
            Err(e) => {
                warn!("X oEmbed also failed: {e}");
                TweetData {
                    text: String::new(),
                    author_name: String::new(),
                    author_username: String::new(),
                    url: tweet_url.to_string(),
                    like_count: None,
                    retweet_count: None,
                    reply_count: None,
                }
            }
        }
    }

    async fn fetch_fxtwitter(&self, username: &str, tweet_id: &str) -> Result<TweetData, String> {
        let url = format!("https://api.fxtwitter.com/{username}/status/{tweet_id}");
        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("request error: {e}"))?;

        if !resp.status().is_success() {
            return Err(format!("fxtwitter returned {}", resp.status()));
        }

        let body: serde_json::Value = resp.json().await.map_err(|e| format!("parse error: {e}"))?;

        let tweet = &body["tweet"];
        let text = tweet["text"].as_str().unwrap_or("").to_string();
        let author = &tweet["author"];
        let author_name = author["name"].as_str().unwrap_or("").to_string();
        let author_username = author["screen_name"].as_str().unwrap_or("").to_string();
        let like_count = tweet["likes"].as_u64();
        let retweet_count = tweet["retweets"].as_u64();
        let reply_count = tweet["replies"].as_u64();

        Ok(TweetData {
            text,
            author_name,
            author_username,
            url: format!("https://x.com/{username}/status/{tweet_id}"),
            like_count,
            retweet_count,
            reply_count,
        })
    }

    async fn fetch_api_v2(&self, tweet_id: &str, bearer_token: &str) -> Result<TweetData, String> {
        let url = format!(
            "https://api.twitter.com/2/tweets/{}?expansions=author_id&user.fields=name,username&tweet.fields=public_metrics",
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
        let metrics = &body["data"]["public_metrics"];

        Ok(TweetData {
            text,
            author_name,
            author_username,
            url: format!("https://x.com/i/web/status/{tweet_id}"),
            like_count: metrics["like_count"].as_u64(),
            retweet_count: metrics["retweet_count"].as_u64(),
            reply_count: metrics["reply_count"].as_u64(),
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

        // Extract tweet text from the embedded HTML <p> tag.
        let text = body["html"]
            .as_str()
            .and_then(|html| {
                let start = html.find("<p")?;
                let after_tag = html[start..].find('>')? + start + 1;
                let end = html[after_tag..].find("</p>")? + after_tag;
                let inner = &html[after_tag..end];
                let clean: String = {
                    let mut out = String::with_capacity(inner.len());
                    let mut in_tag = false;
                    for ch in inner.chars() {
                        match ch {
                            '<' => in_tag = true,
                            '>' => in_tag = false,
                            c if !in_tag => out.push(c),
                            _ => {}
                        }
                    }
                    out
                };
                Some(clean)
            })
            .unwrap_or_default();

        Ok(TweetData {
            text,
            author_name,
            author_username: String::new(),
            url: tweet_url.to_string(),
            like_count: None,
            retweet_count: None,
            reply_count: None,
        })
    }
}
