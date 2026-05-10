//! `/api/v1/user/notice/fetch` — operational announcements.
//!
//! Authoritative response is Laravel-paginated (`{ data: [...], total, ... }`),
//! but the plain-array shape shows up in older forks. `fetch_notices` accepts
//! both so we don't have to maintain per-deployment schemas.

use serde::de::Deserializer;
use serde::{Deserialize, Serialize};

use super::HttpClient;
use crate::error::{Result, XboardError};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Notice {
    pub id: i64,
    #[serde(default)]
    pub title: String,
    /// Backend-rendered HTML. The desktop UI MUST render it as plain text
    /// (no `v-html`) — content is staff-authored on the panel and there's
    /// no reason to grant it script execution privileges in the client.
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub img_url: Option<String>,
    /// Tags arrive as either a JSON array or a comma-separated string
    /// depending on the panel build; `de_tags` collapses both.
    #[serde(default, deserialize_with = "de_tags")]
    pub tags: Vec<String>,
    #[serde(default)]
    pub created_at: Option<i64>,
    #[serde(default)]
    pub updated_at: Option<i64>,
}

fn de_tags<'de, D>(d: D) -> std::result::Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let v = serde_json::Value::deserialize(d)?;
    match v {
        serde_json::Value::Null => Ok(Vec::new()),
        serde_json::Value::Array(arr) => Ok(arr
            .into_iter()
            .filter_map(|x| match x {
                serde_json::Value::String(s) => Some(s),
                serde_json::Value::Number(n) => Some(n.to_string()),
                _ => None,
            })
            .collect()),
        serde_json::Value::String(s) => Ok(s
            .split(',')
            .map(|x| x.trim().to_string())
            .filter(|x| !x.is_empty())
            .collect()),
        _ => Ok(Vec::new()),
    }
}

impl HttpClient {
    /// Fetches the list of operational notices. Caller may apply pagination
    /// client-side; this returns everything the backend page handed us.
    pub async fn fetch_notices(&self) -> Result<Vec<Notice>> {
        let raw: serde_json::Value = self.get_json("/api/v1/user/notice/fetch").await?;
        // Two shapes seen in the wild:
        //   1. plain array          → `[ Notice, Notice, ... ]`
        //   2. Laravel paginate     → `{ data: [...], total, current_page, ... }`
        if raw.is_array() {
            return serde_json::from_value(raw).map_err(|e| {
                XboardError::Other(anyhow::anyhow!("notice list parse: {e}"))
            });
        }
        if let Some(inner) = raw.get("data").cloned() {
            return serde_json::from_value(inner).map_err(|e| {
                XboardError::Other(anyhow::anyhow!("notice paginate parse: {e}"))
            });
        }
        Err(XboardError::Other(anyhow::anyhow!(
            "unrecognised /user/notice/fetch payload shape"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn de_tags_handles_array_string_null() {
        #[derive(Deserialize)]
        struct W {
            #[serde(default, deserialize_with = "de_tags")]
            tags: Vec<String>,
        }
        let from_arr: W = serde_json::from_str(r#"{"tags":["a","b"]}"#).unwrap();
        assert_eq!(from_arr.tags, vec!["a", "b"]);

        let from_csv: W = serde_json::from_str(r#"{"tags":"a,b , c"}"#).unwrap();
        assert_eq!(from_csv.tags, vec!["a", "b", "c"]);

        let from_null: W = serde_json::from_str(r#"{"tags":null}"#).unwrap();
        assert!(from_null.tags.is_empty());

        let missing: W = serde_json::from_str(r#"{}"#).unwrap();
        assert!(missing.tags.is_empty());
    }

    #[test]
    fn notice_round_trip_accepts_partial_payload() {
        let raw = r#"{"id": 1, "title": "Hi"}"#;
        let n: Notice = serde_json::from_str(raw).unwrap();
        assert_eq!(n.id, 1);
        assert_eq!(n.title, "Hi");
        assert!(n.content.is_empty());
        assert!(n.tags.is_empty());
    }
}
