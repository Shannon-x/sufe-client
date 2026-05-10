//! macOS-only: install (or upgrade) the `xboard-helper` LaunchDaemon.
//!
//! The flow runs once per machine (or whenever the bundled helper version
//! is newer than what's installed). It pops a single system auth dialog
//! via `osascript … with administrator privileges`, then:
//!
//! 1. copies the bundled helper to `/Library/Application Support/com.xboard.client/xboard-helper`,
//! 2. writes the LaunchDaemon plist to `/Library/LaunchDaemons/com.xboard.client.helper.plist`,
//! 3. boots the daemon so it's reachable on its Unix socket immediately.
//!
//! We use `osascript` rather than SMAppService because (a) it works back to
//! macOS 11, (b) it doesn't require us to opt into the AppKit "login items"
//! tree, and (c) it's the same pattern clash-verge-rev uses, so we can lean
//! on a well-trodden path.

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use tauri::{AppHandle, Manager};
use xboard_core::kernel::launcher::{HelperInstaller, LauncherError};

const HELPER_LABEL: &str = "com.xboard.client.helper";
const INSTALL_DIR: &str = "/Library/Application Support/com.xboard.client";
pub(crate) const INSTALLED_HELPER_PATH: &str =
    "/Library/Application Support/com.xboard.client/xboard-helper";
pub(crate) const PLIST_PATH: &str = "/Library/LaunchDaemons/com.xboard.client.helper.plist";

/// Returns a ready-to-use installer wired to the helper binary that Tauri
/// extracts alongside the app at runtime. `app` is captured so we can
/// resolve the bundled helper path lazily — the binary path is invariant
/// per app launch but cheap to recompute.
pub fn build_installer(app: &AppHandle) -> Arc<dyn HelperInstaller> {
    Arc::new(OsascriptInstaller {
        bundled_helper: bundled_helper_path(app),
    })
}

#[derive(Debug)]
struct OsascriptInstaller {
    /// Path to the helper binary inside the running .app bundle (or the
    /// `target/<profile>/` tree during dev). May be `None` if the bundle
    /// shipped without it — we surface that as `ServiceMissing` rather
    /// than crashing.
    bundled_helper: Option<PathBuf>,
}

#[async_trait]
impl HelperInstaller for OsascriptInstaller {
    async fn install(&self) -> Result<(), LauncherError> {
        let bundled = self.bundled_helper.clone().ok_or_else(|| {
            LauncherError::ServiceMissing(
                "bundled xboard-helper binary missing from the app".into(),
            )
        })?;
        if !bundled.exists() {
            return Err(LauncherError::ServiceMissing(format!(
                "xboard-helper not found at {}",
                bundled.display()
            )));
        }
        let plist = render_plist();
        // Run blocking I/O + osascript in a worker thread so we don't stall
        // the tauri runtime.
        tokio::task::spawn_blocking(move || run_install(&bundled, &plist))
            .await
            .map_err(|e| LauncherError::Other(format!("join: {e}")))??;
        Ok(())
    }

    async fn uninstall(&self) -> Result<(), LauncherError> {
        tokio::task::spawn_blocking(run_uninstall)
            .await
            .map_err(|e| LauncherError::Other(format!("join: {e}")))?
    }
}

fn run_install(bundled: &Path, plist_xml: &str) -> Result<(), LauncherError> {
    // Write the plist + helper to predictable temp paths first; the admin
    // shell script just `cp`s them into place. This keeps the privileged
    // window short and the script readable.
    let tmp_dir = std::env::temp_dir().join("xboard-client-install");
    std::fs::create_dir_all(&tmp_dir).map_err(LauncherError::Io)?;
    let staged_plist = tmp_dir.join("com.xboard.client.helper.plist");
    let staged_helper = tmp_dir.join("xboard-helper");

    std::fs::write(&staged_plist, plist_xml).map_err(LauncherError::Io)?;
    std::fs::copy(bundled, &staged_helper).map_err(LauncherError::Io)?;

    // Build a single shell pipeline so the user only sees one auth prompt.
    let script = format!(
        "/bin/sh -c '\
mkdir -p \"{install_dir}\" && \
cp \"{staged_helper}\" \"{installed_helper}\" && \
chown root:wheel \"{installed_helper}\" && \
chmod 0755 \"{installed_helper}\" && \
cp \"{staged_plist}\" \"{plist_path}\" && \
chown root:wheel \"{plist_path}\" && \
chmod 0644 \"{plist_path}\" && \
( launchctl bootout system \"{plist_path}\" 2>/dev/null || true ) && \
( launchctl bootstrap system \"{plist_path}\" 2>/dev/null || launchctl load -w \"{plist_path}\" )'",
        install_dir = INSTALL_DIR,
        staged_helper = staged_helper.display(),
        installed_helper = INSTALLED_HELPER_PATH,
        staged_plist = staged_plist.display(),
        plist_path = PLIST_PATH,
    );

    run_as_admin(&script)?;

    // Give launchd a moment to wire up the socket before the launcher
    // re-pings.
    std::thread::sleep(std::time::Duration::from_millis(800));
    Ok(())
}

/// Bootout the LaunchDaemon and delete its on-disk artefacts. Idempotent —
/// the `|| true` guards keep this clean for repeat runs even if the user
/// already partially removed things by hand. Same single-prompt UX as
/// install: one osascript invocation with one shell pipeline.
fn run_uninstall() -> Result<(), LauncherError> {
    let script = format!(
        "/bin/sh -c '\
( launchctl bootout system \"{plist_path}\" 2>/dev/null || true ) && \
( launchctl unload \"{plist_path}\" 2>/dev/null || true ) && \
rm -f \"{plist_path}\" && \
rm -f \"{installed_helper}\" && \
rm -f \"{install_dir}/helper.secret\" && \
rmdir \"{install_dir}\" 2>/dev/null || true'",
        plist_path = PLIST_PATH,
        installed_helper = INSTALLED_HELPER_PATH,
        install_dir = INSTALL_DIR,
    );
    run_as_admin(&script)
}

/// Run a shell script with one administrator-auth prompt. Pulled out of
/// `run_install` so the uninstall path can reuse the prompt + cancellation
/// detection without duplicating the escaping rules.
fn run_as_admin(script: &str) -> Result<(), LauncherError> {
    // osascript needs the script as one literal AppleScript string. The
    // outer escaping is double-quoted; we replace any `"` inside with `\"`
    // so the shell-script literal stays valid.
    let osa_script = format!(
        "do shell script \"{}\" with administrator privileges",
        script.replace('\\', "\\\\").replace('"', "\\\"")
    );

    let output = std::process::Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg(&osa_script)
        .output()
        .map_err(LauncherError::Io)?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        // osascript exits 1 when the user clicks Cancel on the auth dialog
        // and embeds "User canceled" / "(-128)" in stderr.
        if stderr.contains("-128") || stderr.to_lowercase().contains("user canceled") {
            return Err(LauncherError::NeedsConsent(
                "用户拒绝了管理员授权".into(),
            ));
        }
        return Err(LauncherError::Other(format!(
            "osascript exited {}: {}",
            output.status, stderr
        )));
    }
    Ok(())
}

fn render_plist() -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{label}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{installed_helper}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>/var/log/xboard-helper.log</string>
    <key>StandardErrorPath</key>
    <string>/var/log/xboard-helper.log</string>
    <key>EnvironmentVariables</key>
    <dict>
        <key>RUST_LOG</key>
        <string>info</string>
    </dict>
</dict>
</plist>
"#,
        label = HELPER_LABEL,
        installed_helper = INSTALLED_HELPER_PATH,
    )
}

/// Tauri 2 places `externalBin` entries next to the main executable in
/// production (`Contents/MacOS/xboard-helper`) and at
/// `target/<profile>/binaries/xboard-helper-<triple>` during `tauri dev`.
/// Probe both shapes; return the first match. The shell plugin's
/// `sidecar()` would cover this, but we deliberately don't depend on it
/// for the helper because we don't want the shell scope to whitelist the
/// helper for direct invocation from the UI.
fn bundled_helper_path(app: &AppHandle) -> Option<PathBuf> {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let candidate = parent.join("xboard-helper");
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }
    if let Ok(resource_root) = app.path().resource_dir() {
        let candidate = resource_root.join("xboard-helper");
        if candidate.exists() {
            return Some(candidate);
        }
    }
    // Dev fallback: `target/debug/binaries/xboard-helper-<triple>` lives
    // beside the workspace, so walk up from the running binary.
    if let Ok(exe) = std::env::current_exe() {
        let mut cur = exe.as_path();
        while let Some(parent) = cur.parent() {
            let candidate = parent.join("binaries");
            if candidate.is_dir() {
                if let Ok(rd) = std::fs::read_dir(&candidate) {
                    for entry in rd.flatten() {
                        let name = entry.file_name();
                        let s = name.to_string_lossy();
                        if s.starts_with("xboard-helper") {
                            return Some(entry.path());
                        }
                    }
                }
            }
            cur = parent;
        }
    }
    None
}
