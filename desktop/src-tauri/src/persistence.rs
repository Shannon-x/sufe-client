//! Non-secret preferences (login email, subscribe token, theme, last
//! `checkLogin` timestamp). Wraps `tauri-plugin-store` so the rest of the
//! Tauri shell talks plain `SessionSnapshot` instead of raw JSON.
//!
//! The Sanctum bearer never lands here — that goes through `SecureStore`
//! (OS keychain). Anything stored in this file is considered low-sensitivity
//! and a curious user reading the JSON shouldn't be able to assume an
//! authenticated session.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Wry};
use tauri_plugin_store::{Store, StoreExt};

use crate::error::CommandError;

const PREFS_FILE: &str = "preferences.json";
const SESSION_KEY: &str = "session";

/// Cold-start hydration payload. `backend_base_url` is included so the
/// hydrate path can detect a re-pointed backend and discard the snapshot
/// rather than attempt to use credentials against a different server.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionSnapshot {
    pub backend_base_url: String,
    pub email: String,
    pub is_admin: bool,
    /// Xboard subscribe token (`?token=...`), NOT the Sanctum bearer.
    /// Safe to keep in plaintext — appears in the subscription URL too.
    pub subscribe_token: String,
    /// Unix milliseconds. Lets `check_login` decide whether a one-off
    /// network failure should drop the session or be tolerated.
    pub last_check_login_at: Option<i64>,
}

#[derive(Clone)]
pub struct Persistence {
    store: Arc<Store<Wry>>,
}

impl std::fmt::Debug for Persistence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Persistence").finish_non_exhaustive()
    }
}

impl Persistence {
    pub fn load(app: &AppHandle) -> Result<Self, CommandError> {
        let store = app
            .store(PREFS_FILE)
            .map_err(|e| CommandError::new("persistence", format!("open prefs: {e}")))?;
        Ok(Self { store })
    }

    pub fn session(&self) -> Option<SessionSnapshot> {
        let v = self.store.get(SESSION_KEY)?;
        serde_json::from_value(v).ok()
    }

    pub fn save_session(&self, snap: &SessionSnapshot) -> Result<(), CommandError> {
        let v = serde_json::to_value(snap)
            .map_err(|e| CommandError::new("persistence", format!("serialize: {e}")))?;
        self.store.set(SESSION_KEY, v);
        self.store
            .save()
            .map_err(|e| CommandError::new("persistence", format!("save prefs: {e}")))
    }

    pub fn clear_session(&self) -> Result<(), CommandError> {
        self.store.delete(SESSION_KEY);
        self.store
            .save()
            .map_err(|e| CommandError::new("persistence", format!("save prefs: {e}")))
    }
}
