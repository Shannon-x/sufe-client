//! `/api/v1/user/ticket/*` — support tickets.
//!
//! Two read shapes:
//!
//! - List (`?id=` omitted) returns either a plain array or a Laravel
//!   paginate envelope (`{ data: [...], total }`), same as notice/plan/order.
//! - Detail (`?id=<n>`) returns `{ data: TicketDetail }` — a single object
//!   carrying the message thread.
//!
//! Numeric flag conventions (panel build dependent, document them here so
//! the UI can colour-code consistently):
//!   `level`        — 0 = low, 1 = normal, 2 = high
//!   `status`       — 0 = open, 1 = closed
//!   `reply_status` — 0 = waiting on staff, 1 = waiting on user
//!                    (some forks invert; treat as opaque for sorting only)

use serde::{Deserialize, Serialize};

use super::types::de_truthy;
use super::HttpClient;
use crate::error::{Result, XboardError};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Ticket {
    pub id: i64,
    #[serde(default)]
    pub level: i32,
    #[serde(default)]
    pub reply_status: i32,
    #[serde(default)]
    pub status: i32,
    #[serde(default)]
    pub subject: String,
    #[serde(default)]
    pub last_reply_user_id: Option<i64>,
    #[serde(default)]
    pub created_at: Option<i64>,
    #[serde(default)]
    pub updated_at: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TicketMessage {
    pub id: i64,
    #[serde(default)]
    pub ticket_id: i64,
    #[serde(default)]
    pub user_id: Option<i64>,
    #[serde(default)]
    pub message: String,
    /// Whether the calling user wrote this message (vs. a staff reply).
    /// Backend normalises this server-side so the client doesn't need to
    /// know its own `user_id`. May arrive as bool or 0/1.
    #[serde(default, deserialize_with = "de_truthy")]
    pub is_me: bool,
    #[serde(default)]
    pub created_at: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TicketDetail {
    pub id: i64,
    #[serde(default)]
    pub level: i32,
    #[serde(default)]
    pub reply_status: i32,
    #[serde(default)]
    pub status: i32,
    #[serde(default)]
    pub subject: String,
    /// Conversation thread, oldest → newest.
    #[serde(default)]
    pub message: Vec<TicketMessage>,
    #[serde(default)]
    pub created_at: Option<i64>,
    #[serde(default)]
    pub updated_at: Option<i64>,
}

impl HttpClient {
    pub async fn fetch_tickets(&self) -> Result<Vec<Ticket>> {
        let raw: serde_json::Value = self.get_json("/api/v1/user/ticket/fetch").await?;
        if raw.is_array() {
            return serde_json::from_value(raw)
                .map_err(|e| XboardError::Other(anyhow::anyhow!("ticket list parse: {e}")));
        }
        if let Some(inner) = raw.get("data").cloned() {
            return serde_json::from_value(inner).map_err(|e| {
                XboardError::Other(anyhow::anyhow!("ticket paginate parse: {e}"))
            });
        }
        Err(XboardError::Other(anyhow::anyhow!(
            "unrecognised /user/ticket/fetch list shape"
        )))
    }

    pub async fn fetch_ticket(&self, id: i64) -> Result<TicketDetail> {
        // Detail returns `{ data: { ... } }`. `unwrap_envelope` already
        // strips the outer `{ status, data }` envelope when present, but
        // some forks omit `status` and emit a bare `{ data: ... }`, so
        // we accept both here.
        let raw: serde_json::Value = self
            .get_json(&format!("/api/v1/user/ticket/fetch?id={id}"))
            .await?;
        if let Some(inner) = raw.get("data").cloned() {
            return serde_json::from_value(inner)
                .map_err(|e| XboardError::Other(anyhow::anyhow!("ticket detail parse: {e}")));
        }
        serde_json::from_value(raw)
            .map_err(|e| XboardError::Other(anyhow::anyhow!("ticket detail parse: {e}")))
    }

    pub async fn reply_ticket(&self, id: i64, message: &str) -> Result<()> {
        #[derive(Serialize)]
        struct Body<'a> {
            id: i64,
            message: &'a str,
        }
        let _: serde_json::Value = self
            .post_json("/api/v1/user/ticket/reply", &Body { id, message })
            .await?;
        Ok(())
    }

    pub async fn close_ticket(&self, id: i64) -> Result<()> {
        #[derive(Serialize)]
        struct Body {
            id: i64,
        }
        let _: serde_json::Value = self
            .post_json("/api/v1/user/ticket/close", &Body { id })
            .await?;
        Ok(())
    }

    /// Open a new ticket. Returns the freshly-allocated ticket id when the
    /// backend hands it back so the caller can navigate straight to the
    /// thread; some forks return `true` instead, in which case `None` means
    /// "no id known — caller should re-fetch the list".
    pub async fn save_ticket(
        &self,
        subject: &str,
        level: i32,
        message: &str,
    ) -> Result<Option<i64>> {
        #[derive(Serialize)]
        struct Body<'a> {
            subject: &'a str,
            level: i32,
            message: &'a str,
        }
        let raw: serde_json::Value = self
            .post_json(
                "/api/v1/user/ticket/save",
                &Body { subject, level, message },
            )
            .await?;
        // Three shapes seen in the wild: scalar id, `{ id: N }`, or just `true`.
        if let Some(n) = raw.as_i64() {
            return Ok(Some(n));
        }
        if let Some(id) = raw.get("id").and_then(|v| v.as_i64()) {
            return Ok(Some(id));
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticket_list_item_round_trip_minimal() {
        let raw = r#"{
            "id": 7,
            "subject": "Cannot connect",
            "status": 0,
            "level": 1,
            "reply_status": 1,
            "created_at": 1700000000
        }"#;
        let t: Ticket = serde_json::from_str(raw).unwrap();
        assert_eq!(t.id, 7);
        assert_eq!(t.subject, "Cannot connect");
        assert_eq!(t.status, 0);
        assert_eq!(t.level, 1);
    }

    #[test]
    fn ticket_detail_with_messages() {
        let raw = r#"{
            "id": 7,
            "subject": "Cannot connect",
            "status": 0,
            "message": [
                {"id": 1, "ticket_id": 7, "user_id": 42, "message": "Help", "is_me": true, "created_at": 1700000000},
                {"id": 2, "ticket_id": 7, "user_id": 1, "message": "Restart kernel", "is_me": 0, "created_at": 1700000300}
            ]
        }"#;
        let d: TicketDetail = serde_json::from_str(raw).unwrap();
        assert_eq!(d.id, 7);
        assert_eq!(d.message.len(), 2);
        assert!(d.message[0].is_me);
        assert!(!d.message[1].is_me);
        assert_eq!(d.message[1].message, "Restart kernel");
    }

    #[test]
    fn ticket_detail_accepts_partial_payload() {
        // Minimum the panel might emit on a freshly opened ticket.
        let raw = r#"{"id": 9}"#;
        let d: TicketDetail = serde_json::from_str(raw).unwrap();
        assert_eq!(d.id, 9);
        assert!(d.subject.is_empty());
        assert!(d.message.is_empty());
    }
}
