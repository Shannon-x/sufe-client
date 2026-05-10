//! Translate a mihomo subscription YAML into a sing-box 1.10+ JSON config.
//!
//! Used by the iOS NetworkExtension target — Xboard panels only ship mihomo
//! and v2rayng subscriptions, so to run sing-box we fetch the mihomo YAML
//! and rewrite it into sing-box's schema client-side.
//!
//! ## Coverage
//!
//! Protocols translated 1:1:
//!
//! - `ss` → `shadowsocks`
//! - `vmess` → `vmess`
//! - `vless` → `vless`
//! - `trojan` → `trojan`
//! - `hysteria2` → `hysteria2`
//! - `tuic` → `tuic`
//!
//! Anything else (clash-only `hysteria`, `ssr`, `wireguard` w/ mihomo-private
//! keys) is dropped with a `tracing::warn!` and excluded from `outbounds`.
//! Proxy-groups referring to the dropped node are auto-cleaned so sing-box
//! doesn't fail with `outbound not found`.
//!
//! ## Inbound
//!
//! Always emits one `tun` inbound (when `mode=Tun`) — iOS NE *requires* a
//! TUN; SystemProxy mode falls back to a `mixed` inbound on `mixed_port`.
//!
//! ## Why not run sing-box's own clash-config converter
//!
//! sing-box's `convert.go` only handles a fraction of clash dialects (no
//! `proxy-providers`, no `rule-providers`, no Xboard's `dialer-proxy`
//! shorthand). We do the conversion ourselves and stay aligned with the
//! mihomo subset Xboard panels actually emit.

use std::collections::HashSet;

use serde_json::{json, Map, Value};
use serde_yaml::Value as YamlValue;

use crate::error::{Result, XboardError};
use crate::profile::inject::TunnelMode;

/// Render a sing-box JSON config from a mihomo YAML subscription.
///
/// * `yaml` — the subscription text (already fetched). Empty = skeleton.
/// * `external_controller` — `host:port` for the clash-api compat surface
///   sing-box exposes (we read live state through it, just like mihomo).
/// * `secret` — bearer for the clash-api endpoint. Random per session.
/// * `mixed_port` — local mixed inbound for SystemProxy mode (also used as
///   the listen port of the `mixed` inbound when `mode=SystemProxy`).
/// * `mode` — `Tun` emits a `tun` inbound (iOS PacketTunnelProvider feeds
///   the fd back via `LibboxBoxService`); `SystemProxy` emits a `mixed`
///   inbound only.
pub fn patch_singbox(
    yaml: &str,
    external_controller: &str,
    secret: &str,
    mixed_port: u16,
    mode: TunnelMode,
) -> Result<String> {
    let yaml_root = if yaml.trim().is_empty() {
        YamlValue::Mapping(serde_yaml::Mapping::new())
    } else {
        serde_yaml::from_str::<YamlValue>(yaml)?
    };
    let yaml_doc = match yaml_root {
        YamlValue::Mapping(m) => m,
        YamlValue::Null => serde_yaml::Mapping::new(),
        other => {
            return Err(XboardError::Config(format!(
                "expected mihomo YAML root mapping, got {:?}",
                other
            )));
        }
    };

    // ---------- outbounds (proxies + groups) ----------
    let mut outbounds: Vec<Value> = Vec::new();
    let mut accepted: HashSet<String> = HashSet::new();

    if let Some(YamlValue::Sequence(seq)) = yaml_doc.get(YamlValue::String("proxies".into())) {
        for proxy in seq {
            match translate_proxy(proxy) {
                Some(v) => {
                    if let Some(name) = v.get("tag").and_then(Value::as_str) {
                        accepted.insert(name.to_string());
                    }
                    outbounds.push(v);
                }
                None => {
                    tracing::warn!(?proxy, "sing-box: skipping unsupported proxy");
                }
            }
        }
    }

    // The two reserved names sing-box always recognises — used as fall-throughs
    // in selector groups and the route's `final` field.
    accepted.insert("direct".into());
    accepted.insert("block".into());
    outbounds.push(json!({ "type": "direct", "tag": "direct" }));
    outbounds.push(json!({ "type": "block", "tag": "block" }));

    let mut selector_targets: Vec<String> = Vec::new();
    if let Some(YamlValue::Sequence(groups)) = yaml_doc.get(YamlValue::String("proxy-groups".into()))
    {
        for g in groups {
            if let Some((tag, value)) = translate_group(g, &accepted) {
                selector_targets.push(tag);
                outbounds.push(value);
            }
        }
    }

    // The "auto" selector the Connect screen flips through. Falls back to
    // the first translated proxy-group, then to the first concrete proxy.
    let default_target = selector_targets
        .first()
        .cloned()
        .or_else(|| {
            outbounds
                .iter()
                .find(|v| {
                    v.get("type")
                        .and_then(Value::as_str)
                        .map(|t| !["direct", "block", "selector", "urltest"].contains(&t))
                        .unwrap_or(false)
                })
                .and_then(|v| v.get("tag").and_then(Value::as_str).map(str::to_string))
        })
        .unwrap_or_else(|| "direct".into());

    // ---------- route.rules ----------
    let mut rules: Vec<Value> = Vec::new();
    let user_rules = yaml_doc.get(YamlValue::String("rules".into()));
    let translated = translate_rules(user_rules, &accepted, &default_target);
    if translated.is_empty() {
        rules.extend(default_route_rules(&default_target));
    } else {
        rules.extend(translated);
    }

    // ---------- inbounds ----------
    let mut inbounds: Vec<Value> = Vec::new();
    match mode {
        TunnelMode::Tun => {
            // iOS NE: sing-box's auto-detect-interface is a no-op (the
            // tunnel IS the interface). We let Libbox set the fd via
            // `tun.platform.fd_supported` — the Swift side fills `fd`.
            inbounds.push(json!({
                "type": "tun",
                "tag": "tun-in",
                "interface_name": "utun-xboard",
                "address": ["172.19.0.1/30", "fdfe:dcba:9876::1/126"],
                "mtu": 1500,
                "auto_route": true,
                "strict_route": true,
                "stack": "mixed",
                "sniff": true,
                "endpoint_independent_nat": true,
            }));
            inbounds.push(json!({
                "type": "mixed",
                "tag": "mixed-in",
                "listen": "127.0.0.1",
                "listen_port": mixed_port,
                "sniff": true,
            }));
        }
        TunnelMode::SystemProxy => {
            inbounds.push(json!({
                "type": "mixed",
                "tag": "mixed-in",
                "listen": "127.0.0.1",
                "listen_port": mixed_port,
                "sniff": true,
            }));
        }
    }

    // ---------- experimental.clash_api ----------
    // sing-box ≥1.5 exposes a clash-compatible HTTP API under the
    // `experimental.clash_api` key. Our clash-api client (used for stats /
    // proxy switching) treats this identically to mihomo's controller.
    let experimental = json!({
        "clash_api": {
            "external_controller": external_controller,
            "secret": secret,
            "default_mode": "rule",
        },
        "cache_file": {
            "enabled": false,
        },
    });

    // ---------- final assembly ----------
    let root = json!({
        "log": { "level": "info", "timestamp": true },
        "dns": default_dns(),
        "inbounds": inbounds,
        "outbounds": outbounds,
        "route": {
            "rules": rules,
            "auto_detect_interface": true,
            "final": default_target,
        },
        "experimental": experimental,
    });

    Ok(serde_json::to_string_pretty(&root)?)
}

// ---------------------------------------------------------------------- //
// Proxy translation                                                       //
// ---------------------------------------------------------------------- //

fn translate_proxy(proxy: &YamlValue) -> Option<Value> {
    let map = proxy.as_mapping()?;
    let name = ystr(map, "name")?.to_string();
    let server = ystr(map, "server")?.to_string();
    let port = yu16(map, "port")?;
    let kind = ystr(map, "type")?.to_ascii_lowercase();

    match kind.as_str() {
        "ss" | "shadowsocks" => Some(json!({
            "type": "shadowsocks",
            "tag": name,
            "server": server,
            "server_port": port,
            "method": ystr(map, "cipher").unwrap_or("aes-128-gcm"),
            "password": ystr(map, "password").unwrap_or(""),
        })),
        "vmess" => {
            let mut o = Map::new();
            o.insert("type".into(), json!("vmess"));
            o.insert("tag".into(), json!(name));
            o.insert("server".into(), json!(server));
            o.insert("server_port".into(), json!(port));
            if let Some(uuid) = ystr(map, "uuid") {
                o.insert("uuid".into(), json!(uuid));
            }
            o.insert("security".into(), json!(ystr(map, "cipher").unwrap_or("auto")));
            if let Some(aid) = yu16(map, "alterId") {
                o.insert("alter_id".into(), json!(aid));
            }
            if let Some(transport) = translate_transport(map) {
                o.insert("transport".into(), transport);
            }
            if let Some(tls) = translate_tls(map) {
                o.insert("tls".into(), tls);
            }
            Some(Value::Object(o))
        }
        "vless" => {
            let mut o = Map::new();
            o.insert("type".into(), json!("vless"));
            o.insert("tag".into(), json!(name));
            o.insert("server".into(), json!(server));
            o.insert("server_port".into(), json!(port));
            if let Some(uuid) = ystr(map, "uuid") {
                o.insert("uuid".into(), json!(uuid));
            }
            // VLESS flow ("xtls-rprx-vision" etc.) — pass through verbatim.
            if let Some(flow) = ystr(map, "flow") {
                if !flow.is_empty() {
                    o.insert("flow".into(), json!(flow));
                }
            }
            if let Some(transport) = translate_transport(map) {
                o.insert("transport".into(), transport);
            }
            if let Some(tls) = translate_tls(map) {
                o.insert("tls".into(), tls);
            }
            Some(Value::Object(o))
        }
        "trojan" => {
            let mut o = Map::new();
            o.insert("type".into(), json!("trojan"));
            o.insert("tag".into(), json!(name));
            o.insert("server".into(), json!(server));
            o.insert("server_port".into(), json!(port));
            if let Some(pw) = ystr(map, "password") {
                o.insert("password".into(), json!(pw));
            }
            if let Some(transport) = translate_transport(map) {
                o.insert("transport".into(), transport);
            }
            // Trojan implies TLS; if the YAML omits the block, synthesize one
            // so sing-box doesn't refuse the outbound for missing TLS config.
            o.insert(
                "tls".into(),
                translate_tls(map).unwrap_or_else(|| json!({ "enabled": true })),
            );
            Some(Value::Object(o))
        }
        "hysteria2" | "hy2" => {
            let mut o = Map::new();
            o.insert("type".into(), json!("hysteria2"));
            o.insert("tag".into(), json!(name));
            o.insert("server".into(), json!(server));
            o.insert("server_port".into(), json!(port));
            if let Some(pw) = ystr(map, "password") {
                o.insert("password".into(), json!(pw));
            }
            if let Some(up) = ystr(map, "up") {
                o.insert("up_mbps".into(), json!(parse_mbps(up)));
            }
            if let Some(down) = ystr(map, "down") {
                o.insert("down_mbps".into(), json!(parse_mbps(down)));
            }
            if let Some(obfs) = ystr(map, "obfs") {
                let mut obfs_obj = Map::new();
                obfs_obj.insert("type".into(), json!(obfs));
                if let Some(pw) = ystr(map, "obfs-password") {
                    obfs_obj.insert("password".into(), json!(pw));
                }
                o.insert("obfs".into(), Value::Object(obfs_obj));
            }
            o.insert(
                "tls".into(),
                translate_tls(map).unwrap_or_else(|| json!({ "enabled": true })),
            );
            Some(Value::Object(o))
        }
        "tuic" => {
            let mut o = Map::new();
            o.insert("type".into(), json!("tuic"));
            o.insert("tag".into(), json!(name));
            o.insert("server".into(), json!(server));
            o.insert("server_port".into(), json!(port));
            if let Some(uuid) = ystr(map, "uuid") {
                o.insert("uuid".into(), json!(uuid));
            }
            if let Some(pw) = ystr(map, "password") {
                o.insert("password".into(), json!(pw));
            }
            if let Some(cc) = ystr(map, "congestion-controller") {
                o.insert("congestion_control".into(), json!(cc));
            }
            o.insert(
                "tls".into(),
                translate_tls(map).unwrap_or_else(|| json!({ "enabled": true })),
            );
            Some(Value::Object(o))
        }
        _ => None,
    }
}

fn translate_transport(map: &serde_yaml::Mapping) -> Option<Value> {
    let network = ystr(map, "network").unwrap_or("");
    match network {
        "ws" => {
            let opts = map.get(YamlValue::String("ws-opts".into())).and_then(YamlValue::as_mapping);
            let mut t = Map::new();
            t.insert("type".into(), json!("ws"));
            if let Some(o) = opts {
                if let Some(path) = ystr(o, "path") {
                    t.insert("path".into(), json!(path));
                }
                if let Some(YamlValue::Mapping(headers)) =
                    o.get(YamlValue::String("headers".into()))
                {
                    let mut h = Map::new();
                    for (k, v) in headers {
                        if let (Some(k), Some(v)) = (k.as_str(), v.as_str()) {
                            h.insert(k.into(), json!(v));
                        }
                    }
                    if !h.is_empty() {
                        t.insert("headers".into(), Value::Object(h));
                    }
                }
            }
            Some(Value::Object(t))
        }
        "grpc" => {
            let opts = map
                .get(YamlValue::String("grpc-opts".into()))
                .and_then(YamlValue::as_mapping);
            let mut t = Map::new();
            t.insert("type".into(), json!("grpc"));
            if let Some(o) = opts {
                if let Some(svc) = ystr(o, "grpc-service-name") {
                    t.insert("service_name".into(), json!(svc));
                }
            }
            Some(Value::Object(t))
        }
        "http" | "h2" => Some(json!({ "type": "http" })),
        _ => None,
    }
}

fn translate_tls(map: &serde_yaml::Mapping) -> Option<Value> {
    let want = map
        .get(YamlValue::String("tls".into()))
        .and_then(YamlValue::as_bool)
        .unwrap_or(false)
        || ystr(map, "type").map(|s| s == "trojan" || s == "hysteria2" || s == "tuic")
            .unwrap_or(false);
    if !want {
        return None;
    }
    let mut t = Map::new();
    t.insert("enabled".into(), json!(true));
    if let Some(sni) = ystr(map, "servername").or_else(|| ystr(map, "sni")) {
        t.insert("server_name".into(), json!(sni));
    }
    if let Some(skip) = map
        .get(YamlValue::String("skip-cert-verify".into()))
        .and_then(YamlValue::as_bool)
    {
        t.insert("insecure".into(), json!(skip));
    }
    if let Some(YamlValue::Sequence(alpn)) = map.get(YamlValue::String("alpn".into())) {
        let v: Vec<Value> = alpn
            .iter()
            .filter_map(|x| x.as_str().map(|s| json!(s)))
            .collect();
        if !v.is_empty() {
            t.insert("alpn".into(), Value::Array(v));
        }
    }
    // Reality (vless flow=xtls-rprx-vision)
    if let Some(YamlValue::Mapping(reality)) =
        map.get(YamlValue::String("reality-opts".into()))
    {
        let mut r = Map::new();
        r.insert("enabled".into(), json!(true));
        if let Some(pk) = ystr(reality, "public-key") {
            r.insert("public_key".into(), json!(pk));
        }
        if let Some(sid) = ystr(reality, "short-id") {
            r.insert("short_id".into(), json!(sid));
        }
        t.insert("reality".into(), Value::Object(r));
        if let Some(YamlValue::Mapping(client)) = map.get(YamlValue::String("client-fingerprint".into())) {
            // mihomo's "client-fingerprint" is a flat string in modern releases;
            // earlier dialects nested it. Handle both shapes.
            if let Some(fp) = ystr(client, "value") {
                t.insert(
                    "utls".into(),
                    json!({ "enabled": true, "fingerprint": fp }),
                );
            }
        } else if let Some(fp) = ystr(map, "client-fingerprint") {
            t.insert(
                "utls".into(),
                json!({ "enabled": true, "fingerprint": fp }),
            );
        }
    }
    Some(Value::Object(t))
}

// ---------------------------------------------------------------------- //
// Group translation                                                       //
// ---------------------------------------------------------------------- //

fn translate_group(group: &YamlValue, accepted: &HashSet<String>) -> Option<(String, Value)> {
    let map = group.as_mapping()?;
    let name = ystr(map, "name")?.to_string();
    let kind = ystr(map, "type")?.to_ascii_lowercase();
    let proxies = map
        .get(YamlValue::String("proxies".into()))?
        .as_sequence()?;
    let outbounds: Vec<String> = proxies
        .iter()
        .filter_map(|p| p.as_str())
        .filter(|n| accepted.contains(*n))
        .map(str::to_string)
        .collect();
    if outbounds.is_empty() {
        return None;
    }

    match kind.as_str() {
        "select" => Some((
            name.clone(),
            json!({
                "type": "selector",
                "tag": name,
                "outbounds": outbounds,
                "default": outbounds[0].clone(),
            }),
        )),
        "url-test" | "fallback" | "load-balance" => {
            let url = ystr(map, "url").unwrap_or("https://www.gstatic.com/generate_204");
            // mihomo accepts `interval: 600` (number, default) or `interval: 600s`
            // (legacy string form). Sing-box wants `"600s"`.
            let interval_secs = map
                .get(YamlValue::String("interval".into()))
                .and_then(|v| {
                    v.as_u64()
                        .map(|n| n as u32)
                        .or_else(|| v.as_str()?.trim_end_matches('s').parse::<u32>().ok())
                })
                .unwrap_or(300);
            Some((
                name.clone(),
                json!({
                    "type": "urltest",
                    "tag": name,
                    "outbounds": outbounds,
                    "url": url,
                    "interval": format!("{interval_secs}s"),
                }),
            ))
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------- //
// Rule translation                                                        //
// ---------------------------------------------------------------------- //

fn translate_rules(
    rules: Option<&YamlValue>,
    accepted: &HashSet<String>,
    default_target: &str,
) -> Vec<Value> {
    let Some(YamlValue::Sequence(seq)) = rules else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for r in seq {
        let s = match r.as_str() {
            Some(s) => s.trim(),
            None => continue,
        };
        if s.is_empty() {
            continue;
        }
        let parts: Vec<&str> = s.split(',').map(str::trim).collect();
        // mihomo rule grammar: KEYWORD,VALUE,TARGET[,no-resolve]
        // (MATCH/FINAL drops the VALUE column.)
        let head = parts[0].to_ascii_uppercase();
        let (key, value, target) = match head.as_str() {
            "MATCH" | "FINAL" => {
                if parts.len() < 2 {
                    continue;
                }
                ("match", "", parts[1])
            }
            _ => {
                if parts.len() < 3 {
                    continue;
                }
                (
                    sb_rule_key(&head).unwrap_or(""),
                    parts[1],
                    parts[2],
                )
            }
        };
        if key.is_empty() && head != "MATCH" && head != "FINAL" {
            continue;
        }
        let outbound = resolve_target(target, accepted, default_target);
        let mut node = Map::new();
        match (head.as_str(), key) {
            ("MATCH", _) | ("FINAL", _) => {
                node.insert("inbound".into(), json!(["tun-in", "mixed-in"]));
            }
            ("DOMAIN-SUFFIX", _) => {
                node.insert("domain_suffix".into(), json!([value]));
            }
            ("DOMAIN", _) => {
                node.insert("domain".into(), json!([value]));
            }
            ("DOMAIN-KEYWORD", _) => {
                node.insert("domain_keyword".into(), json!([value]));
            }
            ("IP-CIDR" | "IP-CIDR6", _) => {
                node.insert("ip_cidr".into(), json!([value]));
            }
            ("GEOIP", _) => {
                node.insert("geoip".into(), json!([value.to_ascii_lowercase()]));
            }
            ("GEOSITE", _) => {
                node.insert("rule_set".into(), json!([format!("geosite-{}", value.to_ascii_lowercase())]));
            }
            ("PROCESS-NAME", _) => {
                node.insert("process_name".into(), json!([value]));
            }
            _ => continue,
        }
        node.insert("outbound".into(), json!(outbound));
        out.push(Value::Object(node));
    }
    out
}

fn sb_rule_key(head: &str) -> Option<&'static str> {
    match head {
        "DOMAIN" => Some("domain"),
        "DOMAIN-SUFFIX" => Some("domain_suffix"),
        "DOMAIN-KEYWORD" => Some("domain_keyword"),
        "IP-CIDR" | "IP-CIDR6" => Some("ip_cidr"),
        "GEOIP" => Some("geoip"),
        "GEOSITE" => Some("geosite"),
        "PROCESS-NAME" => Some("process_name"),
        _ => None,
    }
}

fn resolve_target(t: &str, accepted: &HashSet<String>, fallback: &str) -> String {
    let normalised = match t.to_ascii_uppercase().as_str() {
        "DIRECT" => "direct".to_string(),
        "REJECT" | "BLOCK" => "block".to_string(),
        "PROXY" | "GLOBAL" => fallback.to_string(),
        _ => t.to_string(),
    };
    if accepted.contains(&normalised) {
        normalised
    } else {
        fallback.to_string()
    }
}

fn default_route_rules(default_target: &str) -> Vec<Value> {
    // Mirror mihomo's default shunt: ads → block, private/CN → direct,
    // everything else → first selector. Geosite/geoip names match
    // sing-box's bundled rule-set (geosite-cn, geoip-cn, etc.).
    let _ = default_target; // referenced via final at the route level
    vec![
        json!({ "rule_set": ["geosite-category-ads-all"], "outbound": "block" }),
        json!({ "ip_is_private": true, "outbound": "direct" }),
        json!({ "rule_set": ["geosite-private", "geosite-cn"], "outbound": "direct" }),
        json!({ "geoip": ["cn", "private"], "outbound": "direct" }),
    ]
}

fn default_dns() -> Value {
    json!({
        "servers": [
            { "tag": "google", "address": "https://1.1.1.1/dns-query", "detour": "direct" },
            { "tag": "local",  "address": "https://223.5.5.5/dns-query", "detour": "direct" },
        ],
        "rules": [
            { "rule_set": ["geosite-cn"], "server": "local" },
        ],
        "strategy": "prefer_ipv4",
        "final": "google",
    })
}

// ---------------------------------------------------------------------- //
// Tiny YAML accessors                                                     //
// ---------------------------------------------------------------------- //

fn ystr<'a>(map: &'a serde_yaml::Mapping, key: &str) -> Option<&'a str> {
    map.get(YamlValue::String(key.into())).and_then(YamlValue::as_str)
}

fn yu16(map: &serde_yaml::Mapping, key: &str) -> Option<u16> {
    let v = map.get(YamlValue::String(key.into()))?;
    v.as_u64().and_then(|n| u16::try_from(n).ok())
}

fn parse_mbps(s: &str) -> u32 {
    // mihomo accepts "100 Mbps" / "100" / "100mbps" — sing-box wants a bare integer.
    s.split_whitespace()
        .next()
        .unwrap_or("0")
        .trim_end_matches(|c: char| !c.is_ascii_digit())
        .parse::<u32>()
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(out: &str) -> Value {
        serde_json::from_str(out).expect("valid JSON")
    }

    #[test]
    fn empty_input_emits_skeleton_with_tun_inbound() {
        let out = patch_singbox("", "127.0.0.1:9090", "deadbeef", 7890, TunnelMode::Tun).unwrap();
        let root = parse(&out);
        let inbounds = root["inbounds"].as_array().unwrap();
        assert!(inbounds.iter().any(|i| i["type"] == "tun"));
        assert!(inbounds.iter().any(|i| i["type"] == "mixed"));

        let api = &root["experimental"]["clash_api"];
        assert_eq!(api["external_controller"], "127.0.0.1:9090");
        assert_eq!(api["secret"], "deadbeef");

        // direct + block always present
        let outs = root["outbounds"].as_array().unwrap();
        let tags: Vec<&str> = outs.iter().filter_map(|v| v["tag"].as_str()).collect();
        assert!(tags.contains(&"direct"));
        assert!(tags.contains(&"block"));
    }

    #[test]
    fn system_proxy_mode_omits_tun_inbound() {
        let out = patch_singbox("", "127.0.0.1:9090", "x", 7890, TunnelMode::SystemProxy).unwrap();
        let root = parse(&out);
        let inbounds = root["inbounds"].as_array().unwrap();
        assert_eq!(inbounds.len(), 1);
        assert_eq!(inbounds[0]["type"], "mixed");
        assert_eq!(inbounds[0]["listen_port"], 7890);
    }

    #[test]
    fn ss_proxy_translates() {
        let yaml = r#"
proxies:
  - name: SS-1
    type: ss
    server: example.com
    port: 443
    cipher: chacha20-ietf-poly1305
    password: hello
"#;
        let out = patch_singbox(yaml, "127.0.0.1:9090", "x", 7890, TunnelMode::Tun).unwrap();
        let root = parse(&out);
        let outs = root["outbounds"].as_array().unwrap();
        let ss = outs.iter().find(|v| v["tag"] == "SS-1").expect("ss outbound");
        assert_eq!(ss["type"], "shadowsocks");
        assert_eq!(ss["server"], "example.com");
        assert_eq!(ss["server_port"], 443);
        assert_eq!(ss["method"], "chacha20-ietf-poly1305");
        assert_eq!(ss["password"], "hello");
    }

    #[test]
    fn vless_with_ws_and_tls_round_trip() {
        let yaml = r#"
proxies:
  - name: V-1
    type: vless
    server: edge.example.com
    port: 443
    uuid: 11111111-2222-3333-4444-555555555555
    tls: true
    servername: edge.example.com
    skip-cert-verify: false
    network: ws
    ws-opts:
      path: /vl
      headers:
        Host: edge.example.com
"#;
        let out = patch_singbox(yaml, "127.0.0.1:9090", "x", 7890, TunnelMode::Tun).unwrap();
        let root = parse(&out);
        let v = root["outbounds"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["tag"] == "V-1")
            .expect("vless outbound");
        assert_eq!(v["type"], "vless");
        assert_eq!(v["uuid"], "11111111-2222-3333-4444-555555555555");
        assert_eq!(v["transport"]["type"], "ws");
        assert_eq!(v["transport"]["path"], "/vl");
        assert_eq!(v["transport"]["headers"]["Host"], "edge.example.com");
        assert_eq!(v["tls"]["enabled"], true);
        assert_eq!(v["tls"]["server_name"], "edge.example.com");
    }

    #[test]
    fn unsupported_proxy_is_dropped_and_groups_clean_up() {
        let yaml = r#"
proxies:
  - name: GoodSS
    type: ss
    server: a.com
    port: 1
    cipher: aes-128-gcm
    password: p
  - name: BadSSR
    type: ssr
    server: b.com
    port: 2
proxy-groups:
  - name: PROXY
    type: select
    proxies: [GoodSS, BadSSR]
"#;
        let out = patch_singbox(yaml, "127.0.0.1:9090", "x", 7890, TunnelMode::Tun).unwrap();
        let root = parse(&out);
        let outs = root["outbounds"].as_array().unwrap();
        let group = outs.iter().find(|v| v["tag"] == "PROXY").unwrap();
        let members: Vec<&str> = group["outbounds"].as_array().unwrap().iter()
            .filter_map(|x| x.as_str()).collect();
        assert_eq!(members, vec!["GoodSS"]);
        assert!(outs.iter().all(|v| v["tag"] != "BadSSR"));
    }

    #[test]
    fn url_test_group_translates_to_urltest() {
        let yaml = r#"
proxies:
  - name: A
    type: ss
    server: a
    port: 1
    cipher: aes-128-gcm
    password: p
  - name: B
    type: ss
    server: b
    port: 2
    cipher: aes-128-gcm
    password: p
proxy-groups:
  - name: 自动
    type: url-test
    proxies: [A, B]
    url: https://www.gstatic.com/generate_204
    interval: 600
"#;
        let out = patch_singbox(yaml, "127.0.0.1:9090", "x", 7890, TunnelMode::Tun).unwrap();
        let root = parse(&out);
        let g = root["outbounds"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["tag"] == "自动")
            .expect("group");
        assert_eq!(g["type"], "urltest");
        assert_eq!(g["interval"], "600s");
    }

    #[test]
    fn rules_translate_keywords() {
        let yaml = r#"
proxies:
  - name: P
    type: ss
    server: a
    port: 1
    cipher: aes-128-gcm
    password: p
proxy-groups:
  - name: PROXY
    type: select
    proxies: [P]
rules:
  - DOMAIN-SUFFIX,example.com,PROXY
  - DOMAIN-KEYWORD,bank,DIRECT
  - GEOSITE,cn,DIRECT
  - GEOIP,CN,DIRECT,no-resolve
  - MATCH,PROXY
"#;
        let out = patch_singbox(yaml, "127.0.0.1:9090", "x", 7890, TunnelMode::Tun).unwrap();
        let root = parse(&out);
        let rs = root["route"]["rules"].as_array().unwrap();
        // First non-MATCH rule
        assert_eq!(rs[0]["domain_suffix"][0], "example.com");
        assert_eq!(rs[0]["outbound"], "PROXY");
        assert!(rs.iter().any(|r| r["geoip"][0] == "cn"));
        assert!(rs.iter().any(|r| r["domain_keyword"][0] == "bank"));
    }

    #[test]
    fn missing_rules_produce_default_shunt() {
        let yaml = r#"
proxies:
  - name: P
    type: ss
    server: a
    port: 1
    cipher: aes-128-gcm
    password: p
proxy-groups:
  - name: PROXY
    type: select
    proxies: [P]
"#;
        let out = patch_singbox(yaml, "127.0.0.1:9090", "x", 7890, TunnelMode::Tun).unwrap();
        let root = parse(&out);
        let rs = root["route"]["rules"].as_array().unwrap();
        // Default shunt should mention ads / cn
        assert!(rs.iter().any(|r| r["outbound"] == "block"));
        assert!(rs.iter().any(|r| r["outbound"] == "direct"));
        assert_eq!(root["route"]["final"], "PROXY");
    }

    #[test]
    fn hysteria2_with_obfs_translates() {
        let yaml = r#"
proxies:
  - name: HY
    type: hysteria2
    server: a.com
    port: 443
    password: pwd
    up: 100 Mbps
    down: 500 Mbps
    obfs: salamander
    obfs-password: secret
"#;
        let out = patch_singbox(yaml, "127.0.0.1:9090", "x", 7890, TunnelMode::Tun).unwrap();
        let root = parse(&out);
        let h = root["outbounds"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["tag"] == "HY")
            .unwrap();
        assert_eq!(h["type"], "hysteria2");
        assert_eq!(h["up_mbps"], 100);
        assert_eq!(h["down_mbps"], 500);
        assert_eq!(h["obfs"]["type"], "salamander");
        assert_eq!(h["obfs"]["password"], "secret");
        assert_eq!(h["tls"]["enabled"], true);
    }

    #[test]
    fn non_mapping_root_is_rejected() {
        let err = patch_singbox(
            "- 1\n- 2\n",
            "127.0.0.1:9090",
            "x",
            7890,
            TunnelMode::Tun,
        )
        .unwrap_err();
        assert!(matches!(err, XboardError::Config(_)));
    }
}
