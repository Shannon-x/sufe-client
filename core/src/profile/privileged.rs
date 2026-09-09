//! Rebuild the configuration accepted by a root/SYSTEM kernel service.
//! The UI cannot choose filesystem locations or expose the privileged controller.

use crate::error::{Result, XboardError};
use serde_yaml::{Mapping, Value};
use sha2::{Digest, Sha256};
use std::net::SocketAddr;

pub const MAX_CONFIG_BYTES: usize = 2 * 1024 * 1024;

pub struct PrivilegedConfig {
    pub yaml: String,
    pub public_addr: String,
    pub public_secret: String,
}

impl std::fmt::Debug for PrivilegedConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PrivilegedConfig")
            .field("public_addr", &self.public_addr)
            .field("yaml", &"[REDACTED]")
            .field("public_secret", &"[REDACTED]")
            .finish()
    }
}

fn reject(message: &str) -> XboardError {
    XboardError::Config(format!("TUN 配置不受支持：{message}"))
}
fn key(name: &str) -> Value {
    Value::String(name.into())
}
fn put(map: &mut Mapping, name: &str, value: impl Into<Value>) {
    map.insert(key(name), value.into());
}
fn text<'a>(map: &'a Mapping, name: &str) -> Result<&'a str> {
    map.get(key(name))
        .and_then(Value::as_str)
        .ok_or_else(|| reject(&format!("缺少 {name}")))
}
fn bounded_text(value: &Value, max: usize) -> Result<()> {
    match value.as_str() {
        Some(s) if !s.is_empty() && s.len() <= max && !s.chars().any(char::is_control) => Ok(()),
        _ => Err(reject("字符串无效或过长")),
    }
}
fn local_address(raw: &str) -> Result<SocketAddr> {
    let addr: SocketAddr = raw.parse().map_err(|_| reject("控制地址无效"))?;
    if addr.ip() != std::net::Ipv4Addr::LOCALHOST || addr.port() < 1024 {
        return Err(reject("控制接口必须使用 127.0.0.1 的非特权端口"));
    }
    Ok(addr)
}
fn valid_secret(secret: &str) -> bool {
    (32..=128).contains(&secret.len())
        && secret
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
}
fn check_tree(value: &Value, depth: usize, count: &mut usize) -> Result<()> {
    *count += 1;
    if depth > 24 || *count > 150_000 {
        return Err(reject("配置过于复杂"));
    }
    match value {
        Value::Mapping(map) => {
            for (k, v) in map {
                bounded_text(k, 512)?;
                check_tree(v, depth + 1, count)?;
            }
        }
        Value::Sequence(list) => {
            for v in list {
                check_tree(v, depth + 1, count)?;
            }
        }
        Value::Tagged(_) => return Err(reject("不允许 YAML 标签")),
        Value::String(s) if s.len() > 16_384 => return Err(reject("配置项过长")),
        _ => (),
    }
    Ok(())
}
fn selected(source: &Mapping, keys: &[&str]) -> Mapping {
    keys.iter()
        .filter_map(|k| source.get(key(k)).map(|v| (key(k), v.clone())))
        .collect()
}

/// `private_addr` and `private_secret` are generated exclusively by the service.
/// The caller writes the returned YAML into a newly created, service-owned home.
pub fn prepare_privileged_config(
    input: &str,
    private_addr: &str,
    private_secret: &str,
) -> Result<PrivilegedConfig> {
    if input.len() > MAX_CONFIG_BYTES {
        return Err(reject("配置超过 2 MiB"));
    }
    // Reject YAML graph expansion before deserializing. These metacharacters at
    // token boundaries are reserved; regular quoted wildcard patterns still work.
    let bytes = input.as_bytes();
    for (i, b) in bytes.iter().enumerate() {
        if b"&*!".contains(b)
            && (i == 0 || bytes[i - 1].is_ascii_whitespace() || b"[{},:".contains(&bytes[i - 1]))
        {
            return Err(reject("请使用不含锚点、别名或标签的普通 YAML"));
        }
    }
    let value: Value = serde_yaml::from_str(input)?;
    check_tree(&value, 0, &mut 0)?;
    let source = value
        .as_mapping()
        .ok_or_else(|| reject("根节点必须是映射"))?;
    let public_addr = local_address(text(source, "external-controller")?)?.to_string();
    let private = local_address(private_addr)?.to_string();
    let public_secret = text(source, "secret")?.to_owned();
    if !valid_secret(&public_secret)
        || !valid_secret(private_secret)
        || private_secret == public_secret
        || private == public_addr
    {
        return Err(reject("控制接口凭据无效"));
    }
    for name in [
        "script",
        "external-ui",
        "external-ui-url",
        "external-ui-name",
        "external-controller-unix",
        "external-controller-pipe",
        "external-controller-tls",
        "listeners",
        "tunnels",
        "ebpf",
        "proxy-providers",
    ] {
        if let Some(v) = source.get(key(name)) {
            let empty = v.is_null()
                || v.as_mapping().is_some_and(Mapping::is_empty)
                || v.as_sequence().is_some_and(Vec::is_empty)
                || v.as_str() == Some("");
            if !empty {
                return Err(reject(&format!(
                    "{name} 不能传入提权服务，请由机场提供内联节点配置"
                )));
            }
        }
    }
    let mut result = selected(
        source,
        &[
            "mode",
            "ipv6",
            "tcp-concurrent",
            "unified-delay",
            "keep-alive-interval",
            "keep-alive-idle",
            "disable-keep-alive",
            "global-client-fingerprint",
            "find-process-mode",
            "hosts",
            "rules",
            "sub-rules",
            "sniffer",
            "geo-auto-update",
            "geo-update-interval",
        ],
    );
    // Never copy geox-url, external UI, extra listeners, authentication, NTP
    // setters, TLS key paths, arbitrary cache homes or new unknown top-level keys.
    put(&mut result, "external-controller", private);
    put(&mut result, "secret", private_secret.to_owned());
    put(&mut result, "allow-lan", false);
    put(&mut result, "bind-address", "127.0.0.1");
    put(&mut result, "log-level", "info");
    let mixed_port = source
        .get(key("mixed-port"))
        .and_then(Value::as_u64)
        .unwrap_or(7890);
    if !(1024..=65535).contains(&mixed_port) {
        return Err(reject("代理端口无效"));
    }
    put(&mut result, "mixed-port", mixed_port);
    let profile = serde_yaml::from_str::<Value>("store-selected: true\nstore-fake-ip: true")?;
    put(&mut result, "profile", profile);

    let proxies = source
        .get(key("proxies"))
        .and_then(Value::as_sequence)
        .ok_or_else(|| reject("需要内联 proxies 节点"))?;
    if proxies.is_empty() || proxies.len() > 5000 {
        return Err(reject("节点数量无效"));
    }
    let nodes = proxies
        .iter()
        .map(sanitize_proxy)
        .collect::<Result<Vec<_>>>()?;
    put(&mut result, "proxies", Value::Sequence(nodes));
    if let Some(groups) = source.get(key("proxy-groups")) {
        let list = groups
            .as_sequence()
            .ok_or_else(|| reject("proxy-groups 必须是列表"))?;
        let groups = list
            .iter()
            .map(|v| {
                let m = v.as_mapping().ok_or_else(|| reject("节点组无效"))?;
                let mut g = selected(
                    m,
                    &[
                        "name",
                        "type",
                        "proxies",
                        "url",
                        "interval",
                        "lazy",
                        "timeout",
                        "max-failed-times",
                        "tolerance",
                        "expected-status",
                        "disable-udp",
                        "hidden",
                        "icon",
                        "strategy",
                    ],
                );
                bounded_text(
                    g.get(key("name")).ok_or_else(|| reject("节点组缺少名称"))?,
                    256,
                )?;
                if !matches!(
                    text(&g, "type")?,
                    "select" | "url-test" | "fallback" | "load-balance" | "relay"
                ) {
                    return Err(reject("节点组类型不受支持"));
                }
                if let Some(url) = g.get(key("url")) {
                    validate_http_url(url, true)?;
                }
                // No provider `use`, include-all-provider or filesystem exclusions.
                g.remove(key("icon"));
                Ok(Value::Mapping(g))
            })
            .collect::<Result<Vec<_>>>()?;
        put(&mut result, "proxy-groups", Value::Sequence(groups));
    }
    let tun = source
        .get(key("tun"))
        .and_then(Value::as_mapping)
        .ok_or_else(|| reject("缺少 TUN 设置"))?;
    if tun.get(key("enable")).and_then(Value::as_bool) != Some(true) {
        return Err(reject("提权服务仅用于 TUN"));
    }
    if tun.contains_key(key("file-descriptor")) {
        return Err(reject("不接受外部文件描述符"));
    }
    let mut tun = selected(
        tun,
        &[
            "enable",
            "stack",
            "auto-route",
            "auto-detect-interface",
            "strict-route",
            "mtu",
            "dns-hijack",
            "inet4-address",
            "inet6-address",
            "route-address",
            "route-exclude-address",
        ],
    );
    put(
        &mut tun,
        "device",
        if cfg!(target_os = "macos") {
            "utun1989"
        } else {
            "Sufe"
        },
    );
    put(&mut result, "tun", Value::Mapping(tun));

    if let Some(dns) = source.get(key("dns")) {
        let dns = dns.as_mapping().ok_or_else(|| reject("DNS 设置无效"))?;
        let mut dns = selected(
            dns,
            &[
                "enable",
                "ipv6",
                "enhanced-mode",
                "fake-ip-range",
                "fake-ip-range6",
                "fake-ip-filter",
                "fake-ip-filter-mode",
                "default-nameserver",
                "nameserver",
                "proxy-server-nameserver",
                "direct-nameserver",
                "direct-nameserver-follow-policy",
                "nameserver-policy",
                "fallback",
                "fallback-filter",
                "use-hosts",
                "use-system-hosts",
                "respect-rules",
                "prefer-h3",
                "cache-algorithm",
            ],
        );
        // The DNS listener must not expose a root service to the LAN.
        put(&mut dns, "listen", "127.0.0.1:1053");
        put(&mut result, "dns", Value::Mapping(dns));
    }
    if let Some(providers) = source.get(key("rule-providers")) {
        let providers = providers
            .as_mapping()
            .ok_or_else(|| reject("规则集定义无效"))?;
        let mut clean = Mapping::new();
        for (name, value) in providers {
            bounded_text(name, 256)?;
            let p = value.as_mapping().ok_or_else(|| reject("规则集无效"))?;
            let kind = text(p, "type")?;
            if !matches!(kind, "http" | "inline") {
                return Err(reject("规则集不能读取本地文件"));
            }
            let mut p = selected(
                p,
                &[
                    "type",
                    "behavior",
                    "format",
                    "url",
                    "interval",
                    "payload",
                    "size-limit",
                ],
            );
            if kind == "http" {
                validate_http_url(
                    p.get(key("url")).ok_or_else(|| reject("规则集缺少 URL"))?,
                    false,
                )?;
                let name_hash = hex::encode(Sha256::digest(name.as_str().unwrap().as_bytes()));
                put(&mut p, "path", format!("./rule-providers/{name_hash}.dat"));
                put(&mut p, "size-limit", 20 * 1024 * 1024_u64);
            }
            clean.insert(name.clone(), Value::Mapping(p));
        }
        put(&mut result, "rule-providers", Value::Mapping(clean));
    }
    Ok(PrivilegedConfig {
        yaml: serde_yaml::to_string(&result)?,
        public_addr,
        public_secret,
    })
}

fn validate_http_url(value: &Value, allow_http: bool) -> Result<()> {
    bounded_text(value, 4096)?;
    let url = url::Url::parse(value.as_str().unwrap()).map_err(|_| reject("URL 无效"))?;
    if !(url.scheme() == "https" || (allow_http && url.scheme() == "http"))
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(reject("仅允许受限 HTTP(S) URL"));
    }
    Ok(())
}

fn sanitize_proxy(value: &Value) -> Result<Value> {
    let map = value.as_mapping().ok_or_else(|| reject("节点必须是映射"))?;
    let kind = text(map, "type")?;
    if !matches!(
        kind,
        "ss" | "ssr"
            | "vmess"
            | "vless"
            | "trojan"
            | "hysteria"
            | "hysteria2"
            | "tuic"
            | "anytls"
            | "http"
            | "socks5"
            | "direct"
            | "reject"
    ) {
        return Err(reject("节点协议不支持提权运行"));
    }
    for field in [
        "certificate",
        "private-key",
        "ca",
        "ca-file",
        "certificate-path",
        "private-key-path",
        "client-certificate",
        "client-key",
        "path",
        "file",
        "script",
        "command",
    ] {
        if map.contains_key(key(field)) {
            return Err(reject("节点不能读取本地文件或执行命令"));
        }
    }
    let mut out = selected(
        map,
        &[
            "name",
            "type",
            "server",
            "port",
            "udp",
            "tfo",
            "ip-version",
            "dialer-proxy",
            "password",
            "username",
            "cipher",
            "uuid",
            "alterId",
            "tls",
            "skip-cert-verify",
            "servername",
            "server-name",
            "sni",
            "network",
            "flow",
            "client-fingerprint",
            "fingerprint",
            "alpn",
            "packet-encoding",
            "global-padding",
            "authenticated-length",
            "udp-over-tcp",
            "udp-over-tcp-version",
            "obfs",
            "obfs-param",
            "protocol",
            "protocol-param",
            "auth",
            "auth-str",
            "obfs-password",
            "up",
            "down",
            "up-speed",
            "down-speed",
            "ports",
            "hop-interval",
            "fast-open",
            "disable-mtu-discovery",
            "recv-window-conn",
            "recv-window",
            "token",
            "congestion-controller",
            "udp-relay-mode",
            "heartbeat-interval",
            "reduce-rtt",
            "request-timeout",
            "disable-sni",
            "max-udp-relay-packet-size",
            "idle-session-check-interval",
            "idle-session-timeout",
            "min-idle-session",
        ],
    );
    bounded_text(
        out.get(key("name")).ok_or_else(|| reject("节点缺少名称"))?,
        256,
    )?;
    for (name, fields) in [
        (
            "ws-opts",
            &[
                "path",
                "headers",
                "max-early-data",
                "early-data-header-name",
                "v2ray-http-upgrade",
                "v2ray-http-upgrade-fast-open",
            ][..],
        ),
        ("grpc-opts", &["grpc-service-name"][..]),
        ("http-opts", &["method", "path", "headers"][..]),
        ("h2-opts", &["host", "path"][..]),
        ("reality-opts", &["public-key", "short-id"][..]),
        (
            "smux",
            &[
                "enabled",
                "protocol",
                "max-connections",
                "min-streams",
                "max-streams",
                "padding",
                "statistic",
                "only-tcp",
            ][..],
        ),
    ] {
        if let Some(options) = map.get(key(name)) {
            let m = options
                .as_mapping()
                .ok_or_else(|| reject("节点传输选项无效"))?;
            put(&mut out, name, Value::Mapping(selected(m, fields)));
        }
    }
    if let Some(plugin) = map.get(key("plugin")) {
        if !matches!(
            plugin.as_str(),
            Some("obfs" | "v2ray-plugin" | "shadow-tls")
        ) {
            return Err(reject("节点插件不受支持"));
        }
        put(&mut out, "plugin", plugin.clone());
        if let Some(options) = map.get(key("plugin-opts")) {
            let options = options
                .as_mapping()
                .ok_or_else(|| reject("节点插件参数无效"))?;
            put(
                &mut out,
                "plugin-opts",
                Value::Mapping(selected(
                    options,
                    &[
                        "mode",
                        "host",
                        "path",
                        "tls",
                        "mux",
                        "skip-cert-verify",
                        "version",
                        "password",
                        "fingerprint",
                    ],
                )),
            );
        }
    }
    Ok(Value::Mapping(out))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn input(extra: &str) -> String {
        format!("external-controller: 127.0.0.1:19090\nsecret: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\nmixed-port: 17890\ntun: {{enable: true, auto-route: true}}\nproxies: [{{name: Test, type: socks5, server: 127.0.0.1, port: 9}}]\nrules: ['MATCH,DIRECT']\n{extra}")
    }
    fn prepare(input: &str) -> Result<PrivilegedConfig> {
        prepare_privileged_config(input, "127.0.0.1:19091", "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb")
    }
    #[test]
    fn private_controller_and_paths_are_service_owned() {
        let config=prepare(&input("geox-url: {geoip: 'file:///etc/shadow'}\ndns: {listen: '0.0.0.0:53', nameserver: ['1.1.1.1']}\nprofile: {path: /etc/shadow}" )).unwrap();
        assert_eq!(config.public_addr, "127.0.0.1:19090");
        assert!(!config.yaml.contains("/etc/shadow"));
        assert!(config.yaml.contains("127.0.0.1:19091"));
        assert!(config.yaml.contains("127.0.0.1:1053"));
        assert!(!format!("{config:?}").contains("aaaaaaaa"));
    }
    #[test]
    fn rejects_files_listeners_aliases_and_remote_proxy_providers() {
        for extra in [
            "external-ui: /etc",
            "external-controller-unix: /tmp/open.sock",
            "proxy-providers: {evil: {type: http, url: https://example.com}}",
            "x: &a [a]\ny: *a",
            "listeners: [{type: tun}]",
            "script: {code: evil}",
        ] {
            assert!(prepare(&input(extra)).is_err(), "{extra}");
        }
        assert!(prepare(
            &input("").replace("type: socks5", "certificate: /etc/shadow, type: socks5")
        )
        .is_err());
    }
    #[test]
    fn rule_provider_paths_are_replaced_and_file_urls_denied() {
        let cfg=prepare(&input("rule-providers: {test: {type: http, behavior: domain, url: https://example.com/rules.yaml, path: /etc/cron.d/evil}}" )).unwrap();
        assert!(!cfg.yaml.contains("/etc/cron"));
        assert!(cfg.yaml.contains("./rule-providers/"));
        assert!(prepare(&input(
            "rule-providers: {test: {type: http, url: 'file:///etc/shadow'}}"
        ))
        .is_err());
    }
    #[test]
    fn accepts_inline_ws_reality_without_file_access() {
        let source=input("").replace("type: socks5", "type: vless, uuid: sample, ws-opts: {path: /ws, headers: {Host: example.com}}, reality-opts: {public-key: test, short-id: abcd}");
        let output = prepare(&source).unwrap();
        assert!(output.yaml.contains("/ws"));
        assert!(output.yaml.contains("public-key"));
    }
    #[test]
    fn rejects_public_controller_and_weak_secrets() {
        assert!(prepare(&input("").replace("127.0.0.1:19090", "0.0.0.0:19090")).is_err());
        assert!(prepare(&input("").replace("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "short")).is_err());
        assert!(prepare(&"a".repeat(MAX_CONFIG_BYTES + 1)).is_err());
    }
    #[test]
    #[ignore = "requires SUFE_TEST_MIHOMO pointing to the verified release kernel"]
    fn release_mihomo_accepts_privileged_snapshot() {
        let binary = std::env::var_os("SUFE_TEST_MIHOMO").expect("set SUFE_TEST_MIHOMO");
        let dir = std::env::temp_dir().join(format!("sufe-config-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.yaml");
        std::fs::write(&path, prepare(&input("dns: {enable: false}")).unwrap().yaml).unwrap();
        let mut command = std::process::Command::new(binary);
        command.arg("-t").arg("-d").arg(&dir).arg("-f").arg(&path);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let output = command.output().unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(
            output.status.success(),
            "{} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
