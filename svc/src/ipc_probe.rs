//! Read-only/adversarial probe for an installed service. Never starts a kernel.
use anyhow::{bail, Result};
use std::{path::PathBuf, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::windows::named_pipe::ServerOptions,
};
use xboard_core::kernel::{
    ipc::{Frame, FrameBody, Request, Response, SVC_PIPE_PATH},
    windows_pipe::open_service_pipe,
};

async fn request(request: Request) -> Result<Response> {
    let mut pipe =
        crate::security::as_unprivileged_caller(|| Ok(open_service_pipe(SVC_PIPE_PATH)?))?;
    let mut encoded = serde_json::to_vec(&Frame::request(17, request))?;
    encoded.push(b'\n');
    pipe.write_all(&encoded).await?;
    let mut reader = BufReader::new(pipe.take(64 * 1024));
    let mut line = String::new();
    tokio::time::timeout(Duration::from_secs(10), reader.read_line(&mut line)).await??;
    let Frame {
        id: 17,
        body: FrameBody::Response(response),
    } = serde_json::from_str(&line)?
    else {
        bail!("unexpected probe reply");
    };
    Ok(response)
}

pub fn run() -> Result<()> {
    tokio::runtime::Builder::new_current_thread().enable_all().build()?.block_on(async {
        let denied_instance = crate::security::as_unprivileged_caller(|| Ok(ServerOptions::new().create(SVC_PIPE_PATH).is_err()))?;
        if !denied_instance { bail!("unprivileged user could create a service pipe instance"); }
        let denied_anonymous = crate::security::as_anonymous_caller(|| Ok(open_service_pipe(SVC_PIPE_PATH).is_err()))?;
        if !denied_anonymous { bail!("anonymous user could open service pipe"); }
        match request(Request::Ping).await? { Response::Pong { helper_version } if helper_version == xboard_core::kernel::ipc::service_version() => (), _ => bail!("unexpected service version") }
        let before = request(Request::Status).await?;
        let forbidden = PathBuf::from(r"C:\Windows\System32\cmd.exe");
        if !matches!(request(Request::StartKernel { exec_path:forbidden.clone(),work_dir:forbidden.clone(),cfg_path:forbidden.clone(),log_path:forbidden }).await?, Response::Error {..}) { bail!("legacy privileged paths were accepted"); }
        let unsafe_yaml = format!("external-controller: 127.0.0.1:19199\nsecret: {}\nexternal-ui: C:/Windows/System32\n", "x".repeat(64));
        if !matches!(request(Request::StartKernelV2 { config_yaml:unsafe_yaml }).await?, Response::Error {..}) { bail!("unsafe configuration was accepted"); }
        let after = request(Request::Status).await?;
        if serde_json::to_string(&before)? != serde_json::to_string(&after)? { bail!("rejected request changed kernel status"); }
        println!("{}", serde_json::json!({"authenticated_restricted_user_ping":true,"anonymous_denied":true,"unprivileged_pipe_instance_denied":true,"legacy_paths_denied":true,"unsafe_yaml_denied":true,"kernel_status_unchanged":true}));
        Ok(())
    })
}
