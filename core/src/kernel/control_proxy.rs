//! Restricted control plane for a privileged mihomo process.
//!
//! The UI credential must never authenticate directly to root mihomo: its
//! `/configs`, `/restart`, `/upgrade`, provider and storage APIs can read/write
//! files or replace an executable. This proxy exposes only the operations our
//! driver needs. Upstream addresses/credentials are service-owned inputs.

use std::{convert::Infallible, net::SocketAddr, sync::Arc, time::Duration};

use bytes::Bytes;
use futures::{StreamExt, TryStreamExt};
use http_body_util::{combinators::UnsyncBoxBody, BodyExt, Full, Limited, StreamBody};
use hyper::{
    body::Frame, body::Incoming, service::service_fn, Method, Request, Response, StatusCode,
};
use hyper_util::rt::{TokioIo, TokioTimer};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::{net::TcpListener, sync::Semaphore, task::JoinHandle};
use tokio_util::sync::CancellationToken;

type BoxError = Box<dyn std::error::Error + Send + Sync>;
type Body = UnsyncBoxBody<Bytes, BoxError>;
const MAX_REQUEST: usize = 8 * 1024;
const MAX_RESPONSE: usize = 16 * 1024 * 1024;
const MAX_LINE: usize = 256 * 1024;

/// Cancels the listener and every active HTTP/streaming connection on drop.
pub struct ControlGateway {
    cancellation: CancellationToken,
    task: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for ControlGateway {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ControlGateway")
            .field("stopped", &self.cancellation.is_cancelled())
            .finish_non_exhaustive()
    }
}

impl ControlGateway {
    pub async fn stop(mut self) {
        self.cancellation.cancel();
        if let Some(task) = self.task.take() {
            let _ = task.await;
        }
    }
}

impl Drop for ControlGateway {
    fn drop(&mut self) {
        self.cancellation.cancel();
        // The owner task holds a JoinSet: aborting it drops/aborts every child.
        if let Some(task) = &self.task {
            task.abort();
        }
    }
}

struct Settings {
    client: reqwest::Client,
    public_auth_hash: [u8; 32],
    private_addr: String,
    private_secret: String,
}

/// `private_addr` must have been confirmed to belong to the newly started
/// service-owned child before calling this function (avoid port-squatting).
pub async fn start_gateway(
    public_addr: &str,
    public_secret: &str,
    private_addr: &str,
    private_secret: &str,
) -> Result<ControlGateway, String> {
    let public = loopback(public_addr)?;
    let private = loopback(private_addr)?;
    if public == private {
        return Err("public and private controllers must differ".into());
    }
    for secret in [public_secret, private_secret] {
        if !(32..=256).contains(&secret.len()) || !secret.bytes().all(|b| b.is_ascii_graphic()) {
            return Err("controller credential must contain 32–256 printable ASCII bytes".into());
        }
    }
    if public_secret == private_secret {
        return Err("private controller credential must be independent".into());
    }
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .http1_only()
        .connect_timeout(Duration::from_secs(3))
        .build()
        .map_err(|_| "cannot construct controller transport".to_string())?;
    let listener = TcpListener::bind(public)
        .await
        .map_err(|e| format!("cannot bind public controller: {e}"))?;
    let settings = Arc::new(Settings {
        client,
        public_auth_hash: Sha256::digest(format!("Bearer {public_secret}").as_bytes()).into(),
        private_addr: private.to_string(),
        private_secret: private_secret.into(),
    });
    let cancellation = CancellationToken::new();
    let shutdown = cancellation.clone();
    let task = tokio::spawn(async move {
        let limit = Arc::new(Semaphore::new(32));
        let mut connections = tokio::task::JoinSet::new();
        loop {
            tokio::select! {
                _ = shutdown.cancelled() => break,
                Some(_) = connections.join_next(), if !connections.is_empty() => {},
                accepted = listener.accept() => {
                    let Ok((socket, address)) = accepted else { break };
                    if !address.ip().is_loopback() { continue; }
                    let Ok(permit) = limit.clone().try_acquire_owned() else { continue };
                    let settings = settings.clone();
                    let shutdown = shutdown.clone();
                    connections.spawn(async move {
                        let _permit = permit;
                        let service = service_fn(move |request| serve(request, settings.clone()));
                        let mut http = hyper::server::conn::http1::Builder::new();
                        http.keep_alive(false).max_buf_size(8192).max_headers(32)
                            .timer(TokioTimer::new()).header_read_timeout(Duration::from_secs(10));
                        tokio::select! {
                            _ = shutdown.cancelled() => {},
                            _ = http.serve_connection(TokioIo::new(socket), service) => {},
                        }
                    });
                }
            }
        }
        connections.abort_all();
        while connections.join_next().await.is_some() {}
    });
    Ok(ControlGateway {
        cancellation,
        task: Some(task),
    })
}

fn loopback(value: &str) -> Result<SocketAddr, String> {
    let addr: SocketAddr = value
        .parse()
        .map_err(|_| "invalid controller address".to_string())?;
    if !addr.ip().is_loopback() || addr.port() < 1024 {
        return Err("controller must bind loopback on an unprivileged port".into());
    }
    Ok(addr)
}

#[derive(Debug, PartialEq)]
enum Route {
    Json,
    Config,
    Stream,
    Select,
}

fn classify(method: &Method, path: &str, query: Option<&str>) -> Option<Route> {
    if path.len() > 4096 || !path.starts_with('/') || path.contains('\\') || path.contains("//") {
        return None;
    }
    let segments: Vec<&str> = path.split('/').skip(1).collect();
    // Only node names are percent-encoded; reject an encoded slash/dot that a
    // downstream router might reinterpret as an endpoint or traversal.
    for segment in &segments {
        let decoded = percent_segment(segment)?;
        if decoded.is_empty()
            || decoded == "."
            || decoded == ".."
            || decoded.contains('/')
            || decoded.contains('\\')
            || decoded.chars().any(char::is_control)
        {
            return None;
        }
    }
    match (method, segments.as_slice()) {
        (&Method::GET, ["version" | "proxies" | "rules" | "connections"]) if query.is_none() => {
            Some(Route::Json)
        }
        (&Method::GET, ["configs"]) if query.is_none() => Some(Route::Config),
        (&Method::GET, ["traffic" | "memory"]) if query.is_none() => Some(Route::Stream),
        (&Method::GET, ["logs"]) if log_query(query) => Some(Route::Stream),
        (&Method::GET, ["proxies", _]) if query.is_none() => Some(Route::Json),
        (&Method::GET, ["proxies", _, "delay"]) if delay_query(query) => Some(Route::Json),
        (&Method::PUT, ["proxies", _]) if query.is_none() => Some(Route::Select),
        (&Method::DELETE, ["connections"] | ["connections", _]) if query.is_none() => {
            Some(Route::Json)
        }
        _ => None,
    }
}

fn percent_segment(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let a = char::from(*bytes.get(index + 1)?).to_digit(16)?;
            let b = char::from(*bytes.get(index + 2)?).to_digit(16)?;
            out.push((a * 16 + b) as u8);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    let decoded = String::from_utf8(out).ok()?;
    // Refuse double decoding too (literal '%' in a node name is a rare,
    // deliberate restriction on this privileged boundary).
    if decoded.contains('%') {
        None
    } else {
        Some(decoded)
    }
}

fn log_query(query: Option<&str>) -> bool {
    match query {
        None => true,
        Some(q) => matches!(
            q,
            "level=info" | "level=debug" | "level=warning" | "level=error" | "level=silent"
        ),
    }
}

fn delay_query(query: Option<&str>) -> bool {
    let Some(query) = query else { return false };
    if query.len() > 4096 {
        return false;
    }
    let pairs: Vec<_> = url::form_urlencoded::parse(query.as_bytes()).collect();
    if pairs.len() != 2 {
        return false;
    }
    let mut have_url = false;
    let mut have_timeout = false;
    for (key, value) in pairs {
        match key.as_ref() {
            "url" if !have_url => {
                let Ok(url) = url::Url::parse(&value) else {
                    return false;
                };
                if !matches!(url.scheme(), "http" | "https")
                    || url.host_str().is_none()
                    || !url.username().is_empty()
                    || url.password().is_some()
                    || url.fragment().is_some()
                {
                    return false;
                }
                have_url = true;
            }
            "timeout" if !have_timeout => {
                if !value
                    .parse::<u32>()
                    .is_ok_and(|n| (100..=30_000).contains(&n))
                {
                    return false;
                }
                have_timeout = true;
            }
            _ => return false,
        }
    }
    have_url && have_timeout
}

async fn serve(
    request: Request<Incoming>,
    settings: Arc<Settings>,
) -> Result<Response<Body>, Infallible> {
    Ok(serve_inner(request, settings)
        .await
        .unwrap_or_else(|_| reply(StatusCode::BAD_GATEWAY, "controller unavailable")))
}

async fn serve_inner(
    request: Request<Incoming>,
    settings: Arc<Settings>,
) -> Result<Response<Body>, BoxError> {
    let headers = request.headers();
    if headers.get_all(hyper::header::AUTHORIZATION).iter().count() != 1 {
        return Ok(reply(StatusCode::UNAUTHORIZED, "authentication required"));
    }
    let auth = headers
        .get(hyper::header::AUTHORIZATION)
        .map(|v| v.as_bytes())
        .unwrap_or_default();
    // Hash comparison avoids exposing a prefix oracle on the actual secret.
    let provided: [u8; 32] = Sha256::digest(auth).into();
    if provided != settings.public_auth_hash {
        return Ok(reply(StatusCode::UNAUTHORIZED, "authentication required"));
    }
    if headers.contains_key(hyper::header::UPGRADE)
        || headers.contains_key(hyper::header::ORIGIN)
        || headers.contains_key(hyper::header::CONTENT_ENCODING)
        || request.uri().scheme().is_some()
        || request.uri().authority().is_some()
    {
        return Ok(reply(
            StatusCode::FORBIDDEN,
            "unsupported controller request",
        ));
    }
    let path = request.uri().path().to_string();
    let Some(route) = classify(request.method(), &path, request.uri().query()) else {
        return Ok(reply(StatusCode::FORBIDDEN, "operation not permitted"));
    };
    let target = format!(
        "http://{}{}",
        settings.private_addr,
        request
            .uri()
            .path_and_query()
            .map(|v| v.as_str())
            .unwrap_or("/")
    );
    let method = request.method().clone();
    let body = match tokio::time::timeout(
        Duration::from_secs(10),
        Limited::new(request.into_body(), MAX_REQUEST).collect(),
    )
    .await
    {
        Ok(Ok(value)) => value.to_bytes(),
        _ => return Ok(reply(StatusCode::PAYLOAD_TOO_LARGE, "invalid request body")),
    };
    let mut upstream = settings
        .client
        .request(method, target)
        .bearer_auth(&settings.private_secret);
    if route == Route::Select {
        let Ok(value) = serde_json::from_slice::<Value>(&body) else {
            return Ok(reply(StatusCode::BAD_REQUEST, "invalid selection"));
        };
        let Some(object) = value.as_object() else {
            return Ok(reply(StatusCode::BAD_REQUEST, "invalid selection"));
        };
        let name = object.get("name").and_then(Value::as_str);
        if object.len() != 1
            || name.is_none_or(|name| {
                name.is_empty() || name.len() > 1024 || name.chars().any(char::is_control)
            })
        {
            return Ok(reply(StatusCode::BAD_REQUEST, "invalid selection"));
        }
        upstream = upstream.json(&value);
    } else if !body.is_empty() {
        return Ok(reply(StatusCode::BAD_REQUEST, "request body not permitted"));
    }
    let response = tokio::time::timeout(Duration::from_secs(35), upstream.send()).await??;
    let status = response.status();
    if status.is_redirection() {
        return Ok(reply(
            StatusCode::BAD_GATEWAY,
            "controller redirect rejected",
        ));
    }
    if route == Route::Stream && status.is_success() {
        let input = response.bytes_stream().map_err(std::io::Error::other);
        let reader = tokio_util::io::StreamReader::new(input);
        let buffered = tokio::io::BufReader::new(reader);
        let secret = settings.private_secret.clone();
        let stream =
            futures::stream::try_unfold((buffered, secret), |(mut reader, secret)| async move {
                use tokio::io::{AsyncBufReadExt, AsyncReadExt};
                let mut line = Vec::new();
                let mut limited = (&mut reader).take((MAX_LINE + 1) as u64);
                let count = tokio::time::timeout(
                    Duration::from_secs(120),
                    limited.read_until(b'\n', &mut line),
                )
                .await??;
                if count == 0 {
                    return Ok::<_, BoxError>(None);
                }
                if count > MAX_LINE {
                    return Err("controller stream line limit".into());
                }
                let text = String::from_utf8(line)?.replace(&secret, "[redacted]");
                Ok(Some((Frame::data(Bytes::from(text)), (reader, secret))))
            });
        let body = StreamBody::new(stream).boxed_unsync();
        return Ok(Response::builder()
            .status(status)
            .header("Content-Type", "application/x-ndjson")
            .header("Cache-Control", "no-store")
            .body(body)?);
    }
    let mut data = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = tokio::time::timeout(Duration::from_secs(15), stream.next()).await? {
        let chunk = chunk?;
        if data.len() + chunk.len() > MAX_RESPONSE {
            return Err("controller response limit".into());
        }
        data.extend_from_slice(&chunk);
    }
    let data = if route == Route::Config && status.is_success() {
        let value: Value = serde_json::from_slice(&data)?;
        serde_json::to_vec(&safe_config(&value))?
    } else {
        String::from_utf8(data)?
            .replace(&settings.private_secret, "[redacted]")
            .into_bytes()
    };
    Ok(Response::builder()
        .status(status)
        .header("Content-Type", "application/json")
        .header("Cache-Control", "no-store")
        .body(full(data))?)
}

fn safe_config(value: &Value) -> Value {
    // Do not recursively copy nested objects: future mihomo versions could
    // add paths or credentials there without changing this gateway.
    let mut safe = serde_json::Map::new();
    for key in [
        "port",
        "socks-port",
        "mixed-port",
        "mode",
        "log-level",
        "allow-lan",
        "ipv6",
    ] {
        if let Some(value) = value
            .get(key)
            .filter(|v| v.is_string() || v.is_number() || v.is_boolean())
        {
            safe.insert(key.into(), value.clone());
        }
    }
    if let Some(enabled) = value
        .get("tun")
        .and_then(|v| v.get("enable"))
        .and_then(Value::as_bool)
    {
        safe.insert("tun".into(), serde_json::json!({"enable": enabled}));
    }
    Value::Object(safe)
}

fn full(data: impl Into<Bytes>) -> Body {
    Full::new(data.into())
        .map_err(|never: Infallible| match never {})
        .boxed_unsync()
}

fn reply(status: StatusCode, message: &str) -> Response<Body> {
    let mut response = Response::new(full(serde_json::json!({"message": message}).to_string()));
    *response.status_mut() = status;
    response.headers_mut().insert(
        hyper::header::CONTENT_TYPE,
        hyper::header::HeaderValue::from_static("application/json"),
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn denies_privileged_mutations_and_path_normalization_tricks() {
        for (method, path) in [
            (Method::PUT, "/configs"),
            (Method::PATCH, "/configs"),
            (Method::POST, "/upgrade"),
            (Method::GET, "/storage/private"),
            (Method::GET, "/providers/proxies"),
            (Method::PUT, "/proxies/%2e%2e"),
            (Method::PUT, "/proxies/%252e%252e"),
            (Method::PUT, "/proxies/name%2frestart"),
            (Method::GET, "//version"),
            (Method::GET, "/proxies/a/../configs"),
        ] {
            assert!(classify(&method, path, None).is_none(), "{method} {path}");
        }
        assert!(classify(&Method::GET, "/version", Some("path=/etc/passwd")).is_none());
    }
    #[test]
    fn accepts_only_driver_routes_and_bounded_delay_query() {
        assert_eq!(
            classify(&Method::PUT, "/proxies/%E8%8A%82%E7%82%B9", None),
            Some(Route::Select)
        );
        assert_eq!(
            classify(&Method::GET, "/logs", Some("level=info")),
            Some(Route::Stream)
        );
        assert_eq!(
            classify(&Method::DELETE, "/connections/id", None),
            Some(Route::Json)
        );
        assert!(delay_query(Some(
            "url=https%3A%2F%2Fwww.gstatic.com%2Fgenerate_204&timeout=5000"
        )));
        assert!(!delay_query(Some(
            "url=file%3A%2F%2F%2Fetc%2Fpasswd&timeout=5000"
        )));
        assert!(!delay_query(Some(
            "url=https%3A%2F%2Fa.test&timeout=999999"
        )));
        assert!(!delay_query(Some(
            "url=https%3A%2F%2Fa.test&url=https%3A%2F%2Fb.test"
        )));
    }
    #[test]
    fn config_response_never_discloses_internal_control_plane() {
        let input = serde_json::json!({"mode":"rule","secret":"private-secret","external-controller":"127.0.0.1:1234","external-controller-unix":"/private/state/socket","tun":{"enable":true,"device":"private"},"tls":{"private-key":"key"}});
        assert_eq!(
            safe_config(&input),
            serde_json::json!({"mode":"rule","tun":{"enable":true}})
        );
        assert!(loopback("0.0.0.0:9090").is_err());
        assert!(loopback("127.0.0.1:80").is_err());
    }

    type Calls = Arc<tokio::sync::Mutex<Vec<(Method, String, String)>>>;

    struct TestUpstream {
        addr: String,
        calls: Calls,
        task: JoinHandle<()>,
    }
    impl Drop for TestUpstream {
        fn drop(&mut self) {
            self.task.abort();
        }
    }

    async fn test_upstream(secret: &str) -> TestUpstream {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let calls: Calls = Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let requests = calls.clone();
        let private = secret.to_owned();
        let task = tokio::spawn(async move {
            let mut children = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    Some(_) = children.join_next(), if !children.is_empty() => {},
                    accepted = listener.accept() => {
                        let (socket, _) = accepted.unwrap();
                        let requests = requests.clone();
                        let private = private.clone();
                        children.spawn(async move {
                            let service = service_fn(move |request: Request<Incoming>| {
                                let requests = requests.clone(); let private = private.clone();
                                async move {
                                    let path = request.uri().path().to_string();
                                    requests.lock().await.push((request.method().clone(), path.clone(), request.headers().get("authorization").and_then(|h| h.to_str().ok()).unwrap_or("").to_owned()));
                                    let _ = request.into_body().collect().await;
                                    let response = if path == "/logs" {
                                        // Deliberately split the secret across upstream frames.
                                        let line = serde_json::json!({"type":"info","payload":private}).to_string()+"\n";
                                        let midpoint=line.len()/2;
                                        let chunks: Vec<Result<Frame<Bytes>,BoxError>> = vec![Ok(Frame::data(Bytes::copy_from_slice(&line.as_bytes()[..midpoint]))),Ok(Frame::data(Bytes::copy_from_slice(&line.as_bytes()[midpoint..])))];
                                        Response::new(StreamBody::new(futures::stream::iter(chunks).chain(futures::stream::pending())).boxed_unsync())
                                    } else if path == "/configs" {
                                        Response::new(full(serde_json::json!({"mode":"rule","secret":private,"external-controller":"127.0.0.1:65500","tun":{"enable":true,"device":"root-private"}}).to_string()))
                                    } else { Response::new(full("{\"version\":\"test\"}")) };
                                    Ok::<_,Infallible>(response)
                                }
                            });
                            let _=hyper::server::conn::http1::Builder::new().serve_connection(TokioIo::new(socket),service).await;
                        });
                    }
                }
            }
        });
        TestUpstream { addr, calls, task }
    }

    fn free_address() -> String {
        std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .to_string()
    }

    #[tokio::test]
    async fn http_boundary_reauthenticates_and_blocks_mutations_before_upstream() {
        let public_secret = "a".repeat(64);
        let private_secret = "b".repeat(64);
        let upstream = test_upstream(&private_secret).await;
        let addr = free_address();
        let gateway = start_gateway(&addr, &public_secret, &upstream.addr, &private_secret)
            .await
            .unwrap();
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let base = format!("http://{addr}");
        assert_eq!(
            client
                .get(format!("{base}/version"))
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
        for path in [
            "/configs",
            "/upgrade",
            "/restart",
            "/providers/proxies/example",
            "/storage/private",
        ] {
            let result = client
                .put(format!("{base}{path}"))
                .bearer_auth(&public_secret)
                .json(&serde_json::json!({"path":"/etc/passwd"}))
                .send()
                .await
                .unwrap();
            assert_eq!(result.status(), StatusCode::FORBIDDEN, "{path}");
        }
        let result = client
            .get(format!("{base}/version"))
            .bearer_auth(&public_secret)
            .header("Origin", "https://attacker.invalid")
            .send()
            .await
            .unwrap();
        assert_eq!(result.status(), StatusCode::FORBIDDEN);
        assert!(upstream.calls.lock().await.is_empty());
        let config: Value = client
            .get(format!("{base}/configs"))
            .bearer_auth(&public_secret)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(
            config,
            serde_json::json!({"mode":"rule","tun":{"enable":true}})
        );
        let bad = client
            .put(format!("{base}/proxies/group"))
            .bearer_auth(&public_secret)
            .json(&serde_json::json!({"name":"node","path":"/etc/passwd"}))
            .send()
            .await
            .unwrap();
        assert_eq!(bad.status(), StatusCode::BAD_REQUEST);
        let good = client
            .put(format!("{base}/proxies/group"))
            .bearer_auth(&public_secret)
            .json(&serde_json::json!({"name":"node"}))
            .send()
            .await
            .unwrap();
        assert!(good.status().is_success());
        let calls = upstream.calls.lock().await;
        assert_eq!(calls.len(), 2);
        assert!(calls
            .iter()
            .all(|(_, _, auth)| auth == &format!("Bearer {private_secret}")));
        drop(calls);
        gateway.stop().await;
        // Windows can finish an already queued TCP handshake after closesocket.
        // The stopped gateway must never serve another authenticated request.
        assert!(client
            .get(format!("{base}/version"))
            .bearer_auth(&public_secret)
            .timeout(Duration::from_secs(1))
            .send()
            .await
            .is_err());
    }

    #[tokio::test]
    async fn streaming_redacts_split_secret_and_gateway_stop_closes_active_response() {
        let public_secret = "c".repeat(64);
        let private_secret = "d".repeat(64);
        let upstream = test_upstream(&private_secret).await;
        let addr = free_address();
        let gateway = start_gateway(&addr, &public_secret, &upstream.addr, &private_secret)
            .await
            .unwrap();
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let mut response = client
            .get(format!("http://{addr}/logs?level=info"))
            .bearer_auth(&public_secret)
            .send()
            .await
            .unwrap();
        let frame = tokio::time::timeout(Duration::from_secs(3), response.chunk())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let line = String::from_utf8(frame.to_vec()).unwrap();
        assert!(line.contains("[redacted]"));
        assert!(!line.contains(&private_secret));
        gateway.stop().await;
        let next = tokio::time::timeout(Duration::from_secs(3), response.chunk())
            .await
            .expect("gateway left a streaming task alive");
        assert!(next.is_err() || next.unwrap().is_none());
    }
}
