//! Shared IPC types between the UI process (`HelperSocketLauncher` /
//! `SvcPipeLauncher`) and the privileged side (`xboard-helper` on macOS,
//! `xboard-svc` on Windows).
//!
//! Wire format: newline-delimited JSON. One [`Frame`] per line in each
//! direction, framed by `\n`. `serde_json` never emits embedded newlines
//! in compact mode, so `read_line` on the receiving side is unambiguous.
//!
//! Threat model: the socket lives at a path only readable by trusted
//! groups (root + staff on macOS) — file-system permissions are the
//! authentication boundary. We add an HMAC over the request body using a
//! per-install secret stored at a root-owned path; this guards against a
//! same-machine attacker who somehow obtains group access from racing
//! the connection. For Phase 1 the HMAC is optional (helpers built before
//! the secret rotation may omit it); production builds enable it.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Default Unix-socket path used by `xboard-helper` on macOS. Lives under
/// `/tmp` so launchd recreates it cleanly across reboots; the helper
/// chowns it `root:staff` and chmods it `0660`.
pub const HELPER_SOCKET_PATH: &str = "/tmp/xboard-helper.sock";

/// Default Windows named-pipe path. Phase 2.
pub const SVC_PIPE_PATH: &str = r"\\.\pipe\xboard-client-svc";

/// Where the helper installs its per-install pre-shared secret. Owned by
/// root, mode 0640, group `staff` so the UI process can read it.
pub const HELPER_SECRET_PATH: &str = "/Library/Application Support/com.xboard.client/helper.secret";

/// Single frame on the wire. Both directions use this envelope so the
/// reader can dispatch on `kind` without first peeking at the body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Frame {
    /// Monotonically increasing client-side request id. Server echoes back
    /// the same id so the launcher can pair responses to requests.
    pub id: u64,
    #[serde(flatten)]
    pub body: FrameBody,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FrameBody {
    Request(Request),
    Response(Response),
}

/// What the UI asks the privileged side to do.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    /// Liveness probe. No side effects.
    Ping,
    /// Whether mihomo is currently running under the helper.
    Status,
    /// Spawn mihomo with the given paths. The privileged side MUST verify
    /// (1) `exec_path` is the bundled mihomo (path under our app bundle on
    /// macOS, our Program Files dir on Windows), (2) `cfg_path`/`work_dir`
    /// are inside the user's Application Support directory — never let the
    /// UI ask the helper to spawn arbitrary binaries from arbitrary paths.
    StartKernel {
        exec_path: PathBuf,
        work_dir: PathBuf,
        cfg_path: PathBuf,
        log_path: PathBuf,
    },
    /// Kill the running kernel and wait for it to exit.
    StopKernel,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Response {
    Pong {
        helper_version: String,
    },
    Status {
        running: bool,
        pid: Option<u32>,
    },
    Started {
        pid: u32,
    },
    Stopped,
    Error {
        message: String,
    },
}

impl Frame {
    pub fn request(id: u64, req: Request) -> Self {
        Self {
            id,
            body: FrameBody::Request(req),
        }
    }

    pub fn response(id: u64, resp: Response) -> Self {
        Self {
            id,
            body: FrameBody::Response(resp),
        }
    }

    pub fn into_response(self) -> Option<Response> {
        match self.body {
            FrameBody::Response(r) => Some(r),
            _ => None,
        }
    }

    pub fn into_request(self) -> Option<Request> {
        match self.body {
            FrameBody::Request(r) => Some(r),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_round_trip_request() {
        let f = Frame::request(7, Request::Ping);
        let s = serde_json::to_string(&f).unwrap();
        let back: Frame = serde_json::from_str(&s).unwrap();
        assert_eq!(back.id, 7);
        assert!(matches!(back.into_request(), Some(Request::Ping)));
    }

    #[test]
    fn frame_round_trip_start_kernel() {
        let f = Frame::request(
            42,
            Request::StartKernel {
                exec_path: PathBuf::from("/usr/local/bin/mihomo"),
                work_dir: PathBuf::from("/var/tmp/k"),
                cfg_path: PathBuf::from("/var/tmp/k/config.yaml"),
                log_path: PathBuf::from("/var/tmp/k/mihomo.log"),
            },
        );
        let s = serde_json::to_string(&f).unwrap();
        assert!(!s.contains('\n'), "compact JSON must not contain newlines");
        let back: Frame = serde_json::from_str(&s).unwrap();
        assert_eq!(back.id, 42);
        match back.into_request() {
            Some(Request::StartKernel { exec_path, .. }) => {
                assert_eq!(exec_path, PathBuf::from("/usr/local/bin/mihomo"));
            }
            other => panic!("expected StartKernel, got {:?}", other),
        }
    }

    #[test]
    fn frame_round_trip_response() {
        let f = Frame::response(1, Response::Started { pid: 1234 });
        let s = serde_json::to_string(&f).unwrap();
        let back: Frame = serde_json::from_str(&s).unwrap();
        match back.into_response() {
            Some(Response::Started { pid }) => assert_eq!(pid, 1234),
            other => panic!("expected Started, got {:?}", other),
        }
    }
}
