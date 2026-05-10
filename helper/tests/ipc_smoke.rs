//! Smoke tests against the real `xboard-helper` binary, run as the current
//! user. We bind the default `/tmp/xboard-helper.sock` (chowning to root:staff
//! will warn-and-skip when not root, which is exactly what we want for a
//! same-user test) and exercise the wire protocol end-to-end.
//!
//! macOS only — the socket + chown semantics don't translate cleanly to
//! Linux's tmpfs and aren't worth conditional-compiling for.
//!
//! The test sets `XBOARD_HELPER_ALLOW_NONROOT=1` so the helper skips its
//! `getuid() == 0` startup gate. Production launchd plists never set that
//! env-var, so the production code path always enforces root.

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use xboard_core::kernel::ipc::{Frame, Request, Response, HELPER_SOCKET_PATH};

/// Drive the helper through Ping → StartKernel(disallowed path, expect Error)
/// → StartKernel(stub) → Status → StopKernel → Status. The whole flow shares
/// a single helper child to avoid racing on the fixed unix-socket path
/// between parallel `#[tokio::test]`s.
#[tokio::test]
async fn helper_lifecycle_smoke() {
    // Pre-clean any stale socket from an earlier crashed run; the helper
    // already does this on startup but doing it here as well makes the test
    // deterministic when it's the very first thing to touch the path.
    let _ = std::fs::remove_file(HELPER_SOCKET_PATH);

    let helper_bin = env!("CARGO_BIN_EXE_xboard-helper");
    let mut child = tokio::process::Command::new(helper_bin)
        .env("RUST_LOG", "warn")
        .env("XBOARD_HELPER_ALLOW_NONROOT", "1")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .expect("spawn helper");

    // Wait up to 2s for the socket to appear.
    let mut attempts = 0;
    while !Path::new(HELPER_SOCKET_PATH).exists() && attempts < 40 {
        tokio::time::sleep(Duration::from_millis(50)).await;
        attempts += 1;
    }
    assert!(
        Path::new(HELPER_SOCKET_PATH).exists(),
        "helper never created the socket within 2s"
    );

    let outcome = run_lifecycle().await;

    // Always tear down the helper before asserting, so a failure doesn't
    // leak a child process that holds the socket or leave staged stubs
    // behind in the user's Application Support directory.
    let _ = child.kill().await;
    let _ = child.wait().await;
    let _ = std::fs::remove_file(HELPER_SOCKET_PATH);
    cleanup_staging();

    outcome.expect("lifecycle flow");
}

async fn run_lifecycle() -> Result<(), String> {
    // 1. Ping → Pong { helper_version: <non-empty> }
    match call(1, Request::Ping).await? {
        Response::Pong { helper_version } => {
            if helper_version.is_empty() {
                return Err("helper_version is empty".into());
            }
        }
        other => return Err(format!("expected Pong, got {other:?}")),
    }

    // 2. Negative path: a real binary outside the install allowlist must be
    //    rejected. We stage it under /tmp with the basename `mihomo` so the
    //    basename pin passes — the failure must come from
    //    `is_under_allowed_root`, which is what we want this test to cover.
    //    (Path validation happens before any work_dir creation, so a rejected
    //    call doesn't leave directories behind.)
    let disallowed = stage_disallowed()
        .await
        .map_err(|e| format!("stage disallowed: {e}"))?;
    let bad = call(
        2,
        Request::StartKernel {
            exec_path: disallowed.clone(),
            work_dir: disallowed.parent().unwrap().to_path_buf(),
            cfg_path: disallowed.with_file_name("cfg.yaml"),
            log_path: disallowed.with_file_name("m.log"),
        },
    )
    .await?;
    match bad {
        Response::Error { message } => {
            // Sanity-check the error blames the root allowlist; if it
            // accidentally fell through to a different code path (e.g. spawn
            // failure on a corrupt stub) the test would still pass for the
            // wrong reason.
            if !message.contains("allowed root") {
                return Err(format!(
                    "Error message did not mention allowed root: {message}"
                ));
            }
        }
        other => {
            return Err(format!(
                "expected Error for disallowed path {}, got {other:?}",
                disallowed.display()
            ));
        }
    }

    // 3. StartKernel happy path: stage a stub under the per-user
    //    Application Support directory so it satisfies the allowed-root
    //    check, and name it exactly `mihomo` so it satisfies the basename
    //    pin. CARGO_TARGET_TMPDIR (which the old test used) lives under the
    //    workspace `target/` tree and is NOT in the helper's allowlist
    //    after the M1 hardening.
    let stage = stage_dir();
    let stub = stage.join("mihomo");
    let work_dir = stage.join("work");
    let cfg = work_dir.join("config.yaml");
    let log = work_dir.join("mihomo.log");
    write_stub(&stub).await.map_err(|e| format!("stub: {e}"))?;
    tokio::fs::create_dir_all(&work_dir)
        .await
        .map_err(|e| format!("mkdir work: {e}"))?;

    let started = call(
        3,
        Request::StartKernel {
            exec_path: stub.clone(),
            work_dir: work_dir.clone(),
            cfg_path: cfg.clone(),
            log_path: log.clone(),
        },
    )
    .await?;
    let pid = match started {
        Response::Started { pid } => {
            if pid == 0 {
                return Err("Started.pid is 0".into());
            }
            pid
        }
        Response::Error { message } => return Err(format!("StartKernel errored: {message}")),
        other => return Err(format!("expected Started, got {other:?}")),
    };

    // 4. Status → running=true, pid matches
    match call(4, Request::Status).await? {
        Response::Status { running, pid: p } => {
            if !running {
                return Err("Status.running=false right after Started".into());
            }
            if p != Some(pid) {
                return Err(format!("Status.pid={:?}, want Some({pid})", p));
            }
        }
        other => return Err(format!("expected Status, got {other:?}")),
    }

    // 5. StopKernel → Stopped
    match call(5, Request::StopKernel).await? {
        Response::Stopped => {}
        other => return Err(format!("expected Stopped, got {other:?}")),
    }

    // 6. Status → running=false. `kill_kernel` already calls `wait().await`
    //    so this should be immediate; if running came back true we'd know
    //    the bug is deterministic, not a race.
    match call(6, Request::Status).await? {
        Response::Status { running, .. } => {
            if running {
                return Err("Status.running=true after StopKernel".into());
            }
        }
        other => return Err(format!("expected Status, got {other:?}")),
    }

    Ok(())
}

/// Per-test-process staging dir under
/// `~/Library/Application Support/com.xboard.client.desktop/`, which is one
/// of the helper's allowed install roots. The pid suffix isolates parallel
/// `cargo test` runs if Cargo ever stops serializing them on the same crate.
fn stage_dir() -> PathBuf {
    let home = std::env::var_os("HOME").expect("HOME unset on macOS");
    let mut p = PathBuf::from(home);
    p.push("Library/Application Support/com.xboard.client.desktop");
    p.push(format!("test-binaries-{}", std::process::id()));
    p
}

/// Per-test-process staging dir for the disallowed-path negative case.
/// `/tmp/...` canonicalizes to `/private/tmp/...` on macOS, which doesn't
/// match any allowed-root prefix.
fn disallowed_dir() -> PathBuf {
    PathBuf::from(format!(
        "/tmp/xboard-test-disallowed-{}",
        std::process::id()
    ))
}

async fn stage_disallowed() -> std::io::Result<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let dir = disallowed_dir();
    tokio::fs::create_dir_all(&dir).await?;
    // Same basename as the good path, so the basename pin passes — this
    // forces the failure to come from the allowed-root check.
    let p = dir.join("mihomo");
    tokio::fs::write(&p, b"#!/bin/sh\nexit 0\n").await?;
    let mut perms = std::fs::metadata(&p)?.permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&p, perms)?;
    Ok(p)
}

async fn write_stub(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    // Stay alive deterministically until the parent kills us. We can't use
    // `cat` here because the helper invokes us with `-d <work> -f <cfg>` —
    // mihomo's args, but `cat -d` is invalid on macOS and exits immediately,
    // so a sleep loop that ignores its args is safer.
    tokio::fs::write(path, b"#!/bin/sh\nwhile true; do sleep 1; done\n").await?;
    let mut perms = std::fs::metadata(path)?.permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms)?;
    Ok(())
}

fn cleanup_staging() {
    let _ = std::fs::remove_dir_all(stage_dir());
    let _ = std::fs::remove_dir_all(disallowed_dir());
}

/// Open a fresh connection per call — the helper handles each line
/// independently and that's what the launcher does in production too.
async fn call(id: u64, req: Request) -> Result<Response, String> {
    let label = format!("{:?}", req);
    let fut = async {
        let stream = UnixStream::connect(HELPER_SOCKET_PATH)
            .await
            .map_err(|e| format!("connect: {e}"))?;
        let (rd, mut wr) = stream.into_split();
        let frame = Frame::request(id, req);
        let mut s = serde_json::to_string(&frame).map_err(|e| format!("encode: {e}"))?;
        s.push('\n');
        wr.write_all(s.as_bytes())
            .await
            .map_err(|e| format!("write: {e}"))?;
        wr.shutdown().await.ok();
        let mut buf = String::new();
        BufReader::new(rd)
            .read_line(&mut buf)
            .await
            .map_err(|e| format!("read: {e}"))?;
        let resp: Frame =
            serde_json::from_str(buf.trim_end()).map_err(|e| format!("decode: {e}"))?;
        if resp.id != id {
            return Err(format!("response id={} expected {id}", resp.id));
        }
        resp.into_response()
            .ok_or_else(|| "frame had no response body".to_string())
    };
    match tokio::time::timeout(Duration::from_secs(5), fut).await {
        Ok(r) => r,
        Err(_) => Err(format!("call({label}) timed out after 5s")),
    }
}
