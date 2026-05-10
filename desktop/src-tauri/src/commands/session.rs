//! Session lifecycle commands: cold-start hydration + active `checkLogin`.
//!
//! `hydrate_session` fires on app start *before* the router decides where
//! to land — it pulls a `SessionSnapshot` off disk, looks up the matching
//! Sanctum bearer in the OS keychain, and (if both check out) seats the
//! `HttpClient` so subsequent commands act authenticated without forcing
//! the user back through the login form.
//!
//! `check_login` is the keep-alive probe. It runs once on startup (after
//! hydrate) and again every time the user navigates onto `/login` so a
//! cached-but-revoked token doesn't get to silently fail on the first
//! authenticated call.
//!
//! On unauthorized / clearly logged-out responses we drop the bearer,
//! clear both stores, and emit `xboard://session-expired` so the
//! frontend shows a "session expired" toast.

use secrecy::SecretString;
use tauri::{AppHandle, Emitter, State};
use xboard_core::XboardError;

use crate::commands::auth::LoginSummary;
use crate::error::{CommandError, CommandResult};
use crate::persistence::SessionSnapshot;
use crate::state::{AppState, AuthSession};

/// Service namespace shared across desktop credentials. Reverse-DNS form so
/// the matching keychain entries are easy to find / wipe by hand.
pub const SECURE_SERVICE: &str = "com.xboard.client";

/// Account key shape: `bearer:<host>:<email>`. The host comes from the live
/// `HttpClient::backend_host()` so multi-account *and* multi-backend
/// switching never collide on a single keychain entry.
pub fn account_key(host: &str, email: &str) -> String {
    format!("bearer:{host}:{email}")
}

#[tauri::command]
pub async fn hydrate_session(
    state: State<'_, AppState>,
) -> CommandResult<Option<LoginSummary>> {
    let client = state
        .snapshot_client()
        .ok_or_else(|| CommandError::new("not_initialized", "后端尚未就绪"))?;
    let persistence = match state.snapshot_persistence() {
        Some(p) => p,
        None => return Ok(None),
    };

    let snap = match persistence.session() {
        Some(s) => s,
        None => return Ok(None),
    };

    if snap.backend_base_url != client.backend_base_url() {
        // Backend pointer changed since last login — discard stale snapshot
        // rather than risk replaying an old token at a different deployment.
        let _ = persistence.clear_session();
        return Ok(None);
    }

    let host = client.backend_host();
    let key = account_key(&host, &snap.email);

    // Missing keyring (Linux without dbus, locked Keychain) is not fatal —
    // we just force the user through the login form again.
    let bearer = match state.snapshot_secure() {
        Some(secure) => match secure.get(&key) {
            Ok(Some(b)) => b,
            Ok(None) => {
                let _ = persistence.clear_session();
                return Ok(None);
            }
            Err(e) => {
                tracing::warn!(error = ?e, "secure store unavailable, clearing snapshot");
                let _ = persistence.clear_session();
                return Ok(None);
            }
        },
        None => {
            let _ = persistence.clear_session();
            return Ok(None);
        }
    };

    client.set_bearer(Some(SecretString::from(bearer)));
    let session = AuthSession {
        email: snap.email.clone(),
        is_admin: snap.is_admin,
        subscribe_token: snap.subscribe_token.clone(),
    };
    *state.auth.write() = Some(session.clone());

    Ok(Some(LoginSummary {
        email: session.email,
        is_admin: session.is_admin,
        subscribe_token: session.subscribe_token,
    }))
}

#[tauri::command]
pub async fn check_login(
    app: AppHandle,
    state: State<'_, AppState>,
) -> CommandResult<bool> {
    let client = state
        .snapshot_client()
        .ok_or_else(|| CommandError::new("not_initialized", "后端尚未就绪"))?;

    match client.check_login().await {
        Ok(resp) if resp.is_login => {
            // Refresh the persisted "last validated" timestamp so a later
            // network outage doesn't immediately drop the session.
            if let Some(persistence) = state.snapshot_persistence() {
                if let Some(mut snap) = persistence.session() {
                    snap.last_check_login_at = Some(now_ms());
                    let _ = persistence.save_session(&snap);
                }
            }
            Ok(true)
        }
        Ok(_) => {
            clear_session(&app, &state);
            Ok(false)
        }
        Err(XboardError::Unauthorized) => {
            clear_session(&app, &state);
            Ok(false)
        }
        Err(e) => {
            // Transient network failure. Tolerate up to 24h of staleness
            // before forcing a re-login — anything longer and we may be
            // sitting on a long-revoked token.
            tracing::warn!(error = %e, "check_login failed, evaluating staleness");
            let recently_validated = state
                .snapshot_persistence()
                .and_then(|p| p.session())
                .and_then(|s| s.last_check_login_at)
                .map(|t| now_ms().saturating_sub(t) < 24 * 60 * 60 * 1000)
                .unwrap_or(false);
            if recently_validated {
                Ok(true)
            } else {
                clear_session(&app, &state);
                Ok(false)
            }
        }
    }
}

/// Best-effort tear-down: drops bearer, clears keychain entry, wipes
/// snapshot, broadcasts the expiry event. Each step swallows its own
/// error because partial cleanup is still better than panicking on a
/// corrupted keychain.
pub fn clear_session(app: &AppHandle, state: &State<'_, AppState>) {
    let host_email = {
        let client = state.client.read().clone();
        let auth = state.auth.read().clone();
        match (client.as_ref(), auth.as_ref()) {
            (Some(c), Some(a)) => Some((c.backend_host(), a.email.clone())),
            _ => None,
        }
    };

    if let Some(client) = state.client.read().clone() {
        client.set_bearer(None);
    }

    if let (Some((host, email)), Some(secure)) =
        (host_email.as_ref(), state.snapshot_secure())
    {
        let _ = secure.delete(&account_key(host, email));
    }

    *state.auth.write() = None;

    if let Some(persistence) = state.snapshot_persistence() {
        let _ = persistence.clear_session();
    }

    let _ = app.emit("xboard://session-expired", ());
}

/// Persist a freshly minted session — called from `login` / `register`
/// after a successful auth roundtrip. Keychain failures degrade
/// gracefully: we still write the JSON snapshot so the user's email /
/// admin flag round-trips, just without the bearer cache.
pub fn store_session_after_auth(
    state: &State<'_, AppState>,
    bearer: &str,
    snapshot: SessionSnapshot,
) -> CommandResult<()> {
    let host = state
        .snapshot_client()
        .map(|c| c.backend_host())
        .ok_or_else(|| CommandError::new("not_initialized", "后端尚未就绪"))?;
    let key = account_key(&host, &snapshot.email);

    if let Some(secure) = state.snapshot_secure() {
        if let Err(e) = secure.put(&key, bearer) {
            tracing::warn!(error = ?e, "secure store unavailable, falling back to in-memory only");
        }
    }

    if let Some(persistence) = state.snapshot_persistence() {
        persistence.save_session(&snapshot)?;
    }

    Ok(())
}

fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
