//! Patch a mihomo subscription YAML so that the kernel listens on *our*
//! external-controller endpoint (random per-session secret, fixed mixed-port)
//! and runs in the tunnel mode our `KernelManager` requested.
//!
//! The subscribe-side YAML usually ships with its own `external-controller` /
//! `secret` / `tun:` block — we always overwrite those, both for security
//! (the cached file would otherwise leak the controller secret) and so the
//! controller URL is predictable on our side.

use serde_yaml::{Mapping, Value};

use crate::error::{Result, XboardError};

/// Which on-host transport mihomo should expose. Mirrors
/// `kernel::manager::TunnelMode` — kept here without the cross-import to keep
/// `profile::` independent of `kernel::`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TunnelMode {
    /// mihomo creates a TUN device and captures all egress.
    Tun,
    /// mihomo only listens on `mixed-port`; OS-level proxy is set elsewhere.
    SystemProxy,
}

/// Default TUN device name per platform. macOS requires the name to start
/// with `utun`; common system utuns occupy 0-7 (iCloud Private Relay,
/// Tailscale, etc.) so we pick a high-numbered one to avoid collisions.
fn default_device_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "utun1989"
    } else {
        // Both wintun (Windows) and Linux TUN accept any name; "Mihomo" is
        // recognisable in `ifconfig`/Network Connections panel.
        "Mihomo"
    }
}

/// Rewrite a mihomo YAML in place.
///
/// * `external_controller` — `host:port` we'll bind the controller HTTP API on
///   (typically `127.0.0.1:9090`).
/// * `secret` — the `Authorization: Bearer …` token we'll demand on every
///   controller call. Random per session.
/// * `mixed_port` — listen port for SOCKS/HTTP combined inbound. Used by the
///   `SystemProxy` mode and also exposed in TUN mode for app-specific routing.
/// * `mode` — `Tun` enables the `tun:` block; `SystemProxy` disables it.
pub fn patch_mihomo(
    yaml: &str,
    external_controller: &str,
    secret: &str,
    mixed_port: u16,
    mode: TunnelMode,
) -> Result<String> {
    // Empty input -> start from a blank mapping. The fetcher should never hand
    // us a non-mapping document but be defensive.
    let mut doc: Mapping = if yaml.trim().is_empty() {
        Mapping::new()
    } else {
        match serde_yaml::from_str::<Value>(yaml)? {
            Value::Mapping(m) => m,
            Value::Null => Mapping::new(),
            other => {
                return Err(XboardError::Config(format!(
                    "expected mihomo YAML root mapping, got {:?}",
                    other
                )));
            }
        }
    };

    doc.insert(
        Value::String("external-controller".into()),
        Value::String(external_controller.into()),
    );
    doc.insert(
        Value::String("secret".into()),
        Value::String(secret.into()),
    );
    doc.insert(
        Value::String("mixed-port".into()),
        Value::Number(mixed_port.into()),
    );
    doc.insert(Value::String("allow-lan".into()), Value::Bool(false));
    doc.insert(
        Value::String("log-level".into()),
        Value::String("info".into()),
    );
    // IPv6 must be on, otherwise v6 traffic bypasses TUN and goes direct.
    doc.insert(Value::String("ipv6".into()), Value::Bool(true));

    let mut tun = Mapping::new();
    match mode {
        TunnelMode::Tun => {
            tun.insert(Value::String("enable".into()), Value::Bool(true));
            // `mixed`: TCP via system stack + UDP via gvisor. The default
            // `gvisor` is too slow for large file transfers; pure `system` is
            // the fastest but Windows Defender / macOS firewall sometimes
            // refuses to whitelist it.
            tun.insert(
                Value::String("stack".into()),
                Value::String("mixed".into()),
            );
            tun.insert(Value::String("auto-route".into()), Value::Bool(true));
            tun.insert(
                Value::String("auto-detect-interface".into()),
                Value::Bool(true),
            );
            // strict-route adds firewall rules on Windows so non-TUN egress
            // is blocked outright (key for DNS-leak prevention).
            tun.insert(Value::String("strict-route".into()), Value::Bool(true));
            // 9000 lets the kernel coalesce TCP segments; pairs with `gso` on Linux.
            tun.insert(Value::String("mtu".into()), Value::Number(9000.into()));
            // Hijack BOTH UDP/53 and TCP/53 — DoT and a surprising amount of
            // legacy software fall back to TCP DNS, which `any:53` alone misses.
            tun.insert(
                Value::String("dns-hijack".into()),
                Value::Sequence(vec![
                    Value::String("any:53".into()),
                    Value::String("tcp://any:53".into()),
                ]),
            );
            tun.insert(
                Value::String("device".into()),
                Value::String(default_device_name().into()),
            );

            // Linux-only TUN keys. Emitting these on Windows/macOS makes the
            // mihomo Linux-targeted code-paths panic at YAML parse time.
            #[cfg(target_os = "linux")]
            {
                // Auto-write nftables rules in `output` chain so processes
                // bound to a non-default interface still get redirected.
                tun.insert(
                    Value::String("auto-redirect".into()),
                    Value::Bool(true),
                );
                // Kernel GSO offloading — material throughput improvement on
                // Linux 5.10+, where TUN GSO checksumming is solid.
                tun.insert(Value::String("gso".into()), Value::Bool(true));
                tun.insert(
                    Value::String("gso-max-size".into()),
                    Value::Number(65536.into()),
                );
                // Pick rule/table indices far from systemd-networkd defaults
                // (main=254, default=253, local=255) and common VPN ranges.
                tun.insert(
                    Value::String("iproute2-table-index".into()),
                    Value::Number(2022.into()),
                );
                tun.insert(
                    Value::String("iproute2-rule-index".into()),
                    Value::Number(9000.into()),
                );
            }
        }
        TunnelMode::SystemProxy => {
            tun.insert(Value::String("enable".into()), Value::Bool(false));
        }
    }
    doc.insert(Value::String("tun".into()), Value::Mapping(tun));

    // mihomo's built-in DNS must take over once we hijack 53. `listen: 1053`
    // sidesteps the systemd-resolved 53-port conflict on Linux; the hijack is
    // packet-level so the listen port can be anything mihomo can bind.
    let mut dns = Mapping::new();
    dns.insert(Value::String("enable".into()), Value::Bool(true));
    dns.insert(
        Value::String("listen".into()),
        Value::String("0.0.0.0:1053".into()),
    );
    dns.insert(
        Value::String("enhanced-mode".into()),
        Value::String("fake-ip".into()),
    );
    dns.insert(
        Value::String("fake-ip-range".into()),
        Value::String("198.18.0.1/16".into()),
    );
    dns.insert(
        Value::String("nameserver".into()),
        Value::Sequence(vec![
            Value::String("https://1.1.1.1/dns-query".into()),
            Value::String("https://dns.google/dns-query".into()),
        ]),
    );
    dns.insert(
        Value::String("fallback".into()),
        Value::Sequence(vec![Value::String("https://1.0.0.1/dns-query".into())]),
    );
    doc.insert(Value::String("dns".into()), Value::Mapping(dns));

    // Auto-shunt: if the subscription didn't ship its own routing rules, inject
    // a sensible default — block ads, send CN traffic direct, foreign through
    // the first user-defined Selector group. Subscriptions that *do* have
    // rules are left alone so users get exactly what their backend configured.
    if needs_default_rules(&doc) {
        let proxy_target = pick_default_proxy_target(&doc).unwrap_or_else(|| "GLOBAL".to_string());
        doc.insert(
            Value::String("mode".into()),
            Value::String("rule".into()),
        );
        // Mihomo's built-in GEOSITE / GEOIP matchers read from `geosite.dat` +
        // `geoip.metadb`. If the kernel doesn't yet have them on disk, the
        // default `geo-auto-update: true` (set below) will pull them on first
        // start. No `rule-providers` block is needed.
        doc.insert(
            Value::String("geo-auto-update".into()),
            Value::Bool(true),
        );
        doc.insert(
            Value::String("geo-update-interval".into()),
            Value::Number(24.into()),
        );
        let rules: Vec<String> = vec![
            "GEOSITE,category-ads-all,REJECT".into(),
            "GEOSITE,private,DIRECT".into(),
            "GEOIP,private,DIRECT,no-resolve".into(),
            "GEOIP,lan,DIRECT,no-resolve".into(),
            "GEOSITE,cn,DIRECT".into(),
            "GEOSITE,geolocation-cn,DIRECT".into(),
            "GEOIP,CN,DIRECT,no-resolve".into(),
            // Final catch-all: everything not matched above goes through the
            // user's selected proxy group.
            format!("MATCH,{proxy_target}"),
        ];
        doc.insert(
            Value::String("rules".into()),
            Value::Sequence(rules.into_iter().map(Value::String).collect()),
        );
    }

    let out = serde_yaml::to_string(&Value::Mapping(doc))?;
    Ok(out)
}

/// Returns true when the YAML has no usable `rules` array. We treat an empty
/// sequence and a single `MATCH,DIRECT` (which would defeat the proxy entirely)
/// as "no rules" too.
fn needs_default_rules(doc: &Mapping) -> bool {
    match doc.get(Value::String("rules".into())) {
        None => true,
        Some(Value::Sequence(seq)) => {
            if seq.is_empty() {
                return true;
            }
            // A subscription whose only rule is `MATCH,DIRECT` is effectively
            // unrouted — replace it. (Some test/free Xboard backends ship this.)
            if seq.len() == 1 {
                if let Some(Value::String(s)) = seq.first() {
                    let t = s.trim().to_ascii_uppercase();
                    if t == "MATCH,DIRECT" || t == "FINAL,DIRECT" {
                        return true;
                    }
                }
            }
            false
        }
        Some(_) => true,
    }
}

/// Pick the first user-defined Selector group as the default `MATCH` target.
/// Falls back to `None` (caller uses `GLOBAL`) when no proxy-groups exist or
/// none are of a switchable type.
fn pick_default_proxy_target(doc: &Mapping) -> Option<String> {
    let groups = doc.get(Value::String("proxy-groups".into()))?.as_sequence()?;
    // Prefer the first Selector — that's the group the user actually clicks on.
    if let Some(name) = first_group_of_type(groups, "select") {
        return Some(name);
    }
    // Then URLTest / Fallback / LoadBalance — anything that yields a single
    // working endpoint without user input.
    for ty in ["url-test", "fallback", "load-balance"] {
        if let Some(name) = first_group_of_type(groups, ty) {
            return Some(name);
        }
    }
    // Last resort: first group with any name we can read.
    groups
        .iter()
        .find_map(|v| v.as_mapping()?.get(Value::String("name".into()))?.as_str().map(str::to_string))
}

fn first_group_of_type(groups: &[Value], wanted: &str) -> Option<String> {
    for g in groups {
        let map = match g.as_mapping() {
            Some(m) => m,
            None => continue,
        };
        let ty = map
            .get(Value::String("type".into()))
            .and_then(Value::as_str)
            .unwrap_or("");
        if ty.eq_ignore_ascii_case(wanted) {
            if let Some(name) = map
                .get(Value::String("name".into()))
                .and_then(Value::as_str)
            {
                return Some(name.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(out: &str) -> Mapping {
        match serde_yaml::from_str::<Value>(out).unwrap() {
            Value::Mapping(m) => m,
            other => panic!("expected mapping, got {:?}", other),
        }
    }

    #[test]
    fn empty_input_with_tun_mode_emits_full_tun_block() {
        let out = patch_mihomo("", "127.0.0.1:9090", "deadbeef", 7890, TunnelMode::Tun).unwrap();
        let m = parse(&out);

        assert_eq!(
            m.get(Value::String("external-controller".into()))
                .unwrap()
                .as_str()
                .unwrap(),
            "127.0.0.1:9090"
        );
        assert_eq!(
            m.get(Value::String("secret".into()))
                .unwrap()
                .as_str()
                .unwrap(),
            "deadbeef"
        );
        assert_eq!(
            m.get(Value::String("mixed-port".into()))
                .unwrap()
                .as_u64()
                .unwrap(),
            7890
        );
        assert_eq!(
            m.get(Value::String("ipv6".into())).unwrap(),
            &Value::Bool(true)
        );

        let tun = match m.get(Value::String("tun".into())).unwrap() {
            Value::Mapping(t) => t,
            _ => panic!("tun must be mapping"),
        };
        assert_eq!(
            tun.get(Value::String("enable".into())).unwrap(),
            &Value::Bool(true)
        );
        assert_eq!(
            tun.get(Value::String("stack".into()))
                .unwrap()
                .as_str()
                .unwrap(),
            "mixed"
        );
        assert_eq!(
            tun.get(Value::String("strict-route".into())).unwrap(),
            &Value::Bool(true)
        );
        assert_eq!(
            tun.get(Value::String("mtu".into()))
                .unwrap()
                .as_u64()
                .unwrap(),
            9000
        );
        let hijack = tun
            .get(Value::String("dns-hijack".into()))
            .unwrap()
            .as_sequence()
            .unwrap();
        assert_eq!(hijack.len(), 2);
        assert_eq!(hijack[0].as_str().unwrap(), "any:53");
        assert_eq!(hijack[1].as_str().unwrap(), "tcp://any:53");

        let dns = match m.get(Value::String("dns".into())).unwrap() {
            Value::Mapping(d) => d,
            _ => panic!("dns must be mapping"),
        };
        assert_eq!(
            dns.get(Value::String("enhanced-mode".into()))
                .unwrap()
                .as_str()
                .unwrap(),
            "fake-ip"
        );
        assert_eq!(
            dns.get(Value::String("listen".into()))
                .unwrap()
                .as_str()
                .unwrap(),
            "0.0.0.0:1053"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_device_name_starts_with_utun() {
        let out = patch_mihomo("", "127.0.0.1:9090", "x", 7890, TunnelMode::Tun).unwrap();
        let m = parse(&out);
        let tun = match m.get(Value::String("tun".into())).unwrap() {
            Value::Mapping(t) => t,
            _ => panic!(),
        };
        let device = tun
            .get(Value::String("device".into()))
            .unwrap()
            .as_str()
            .unwrap();
        assert!(
            device.starts_with("utun"),
            "macOS TUN device must start with utun, got {device}"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_emits_iproute2_and_gso_keys() {
        let out = patch_mihomo("", "127.0.0.1:9090", "x", 7890, TunnelMode::Tun).unwrap();
        let m = parse(&out);
        let tun = match m.get(Value::String("tun".into())).unwrap() {
            Value::Mapping(t) => t,
            _ => panic!(),
        };
        assert_eq!(
            tun.get(Value::String("auto-redirect".into())).unwrap(),
            &Value::Bool(true)
        );
        assert_eq!(
            tun.get(Value::String("gso".into())).unwrap(),
            &Value::Bool(true)
        );
        assert_eq!(
            tun.get(Value::String("iproute2-table-index".into()))
                .unwrap()
                .as_u64()
                .unwrap(),
            2022
        );
    }

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    #[test]
    fn non_linux_omits_linux_only_keys() {
        let out = patch_mihomo("", "127.0.0.1:9090", "x", 7890, TunnelMode::Tun).unwrap();
        let m = parse(&out);
        let tun = match m.get(Value::String("tun".into())).unwrap() {
            Value::Mapping(t) => t,
            _ => panic!(),
        };
        assert!(tun.get(Value::String("auto-redirect".into())).is_none());
        assert!(tun.get(Value::String("gso".into())).is_none());
        assert!(tun
            .get(Value::String("iproute2-table-index".into()))
            .is_none());
    }

    #[test]
    fn existing_tun_block_is_disabled_when_system_proxy_mode() {
        let yaml = "tun:\n  enable: true\n  stack: system\n";
        let out =
            patch_mihomo(yaml, "127.0.0.1:9090", "x", 7890, TunnelMode::SystemProxy).unwrap();
        let m = parse(&out);
        let tun = match m.get(Value::String("tun".into())).unwrap() {
            Value::Mapping(t) => t,
            _ => panic!(),
        };
        assert_eq!(
            tun.get(Value::String("enable".into())).unwrap(),
            &Value::Bool(false)
        );
        // Whole sub-block was rewritten — old "stack: system" must be gone.
        assert!(tun.get(Value::String("stack".into())).is_none());
    }

    #[test]
    fn upstream_mixed_port_is_forced_back_to_ours() {
        let yaml = "mixed-port: 1080\nproxies: []\n";
        let out = patch_mihomo(yaml, "127.0.0.1:9090", "s", 7890, TunnelMode::Tun).unwrap();
        let m = parse(&out);
        assert_eq!(
            m.get(Value::String("mixed-port".into()))
                .unwrap()
                .as_u64()
                .unwrap(),
            7890
        );
        // Untouched fields survive.
        assert!(m.get(Value::String("proxies".into())).is_some());
    }

    #[test]
    fn upstream_secret_is_overwritten_with_ours() {
        let yaml = "secret: leaked-from-cache\n";
        let out =
            patch_mihomo(yaml, "127.0.0.1:9090", "fresh-32-byte-hex", 7890, TunnelMode::Tun)
                .unwrap();
        let m = parse(&out);
        assert_eq!(
            m.get(Value::String("secret".into()))
                .unwrap()
                .as_str()
                .unwrap(),
            "fresh-32-byte-hex"
        );
    }

    #[test]
    fn missing_rules_get_default_shunt_with_global_target() {
        let yaml = "proxies: []\n";
        let out = patch_mihomo(yaml, "127.0.0.1:9090", "s", 7890, TunnelMode::Tun).unwrap();
        let m = parse(&out);
        assert_eq!(
            m.get(Value::String("mode".into()))
                .unwrap()
                .as_str()
                .unwrap(),
            "rule"
        );
        let rules = m
            .get(Value::String("rules".into()))
            .unwrap()
            .as_sequence()
            .unwrap();
        assert!(
            rules
                .iter()
                .any(|r| r.as_str() == Some("GEOSITE,category-ads-all,REJECT")),
            "expected ad-reject rule"
        );
        assert!(
            rules
                .iter()
                .any(|r| r.as_str() == Some("GEOSITE,cn,DIRECT")),
            "expected CN-direct rule"
        );
        // No proxy-groups → default target = GLOBAL.
        let last = rules.last().unwrap().as_str().unwrap();
        assert_eq!(last, "MATCH,GLOBAL");
    }

    #[test]
    fn default_shunt_targets_first_selector_group() {
        let yaml = r#"
proxy-groups:
  - name: 自动选择
    type: url-test
    proxies: [a, b]
  - name: PROXY
    type: select
    proxies: [a, b, 自动选择]
  - name: 备用
    type: select
    proxies: [a]
"#;
        let out = patch_mihomo(yaml, "127.0.0.1:9090", "s", 7890, TunnelMode::Tun).unwrap();
        let m = parse(&out);
        let rules = m
            .get(Value::String("rules".into()))
            .unwrap()
            .as_sequence()
            .unwrap();
        let last = rules.last().unwrap().as_str().unwrap();
        assert_eq!(last, "MATCH,PROXY", "should pick first Selector group");
    }

    #[test]
    fn user_supplied_rules_are_preserved() {
        let yaml = r#"
proxy-groups:
  - name: PROXY
    type: select
    proxies: [a]
rules:
  - DOMAIN-SUFFIX,example.com,PROXY
  - DOMAIN-SUFFIX,bank.cn,DIRECT
  - MATCH,PROXY
"#;
        let out = patch_mihomo(yaml, "127.0.0.1:9090", "s", 7890, TunnelMode::Tun).unwrap();
        let m = parse(&out);
        let rules = m
            .get(Value::String("rules".into()))
            .unwrap()
            .as_sequence()
            .unwrap();
        assert_eq!(rules.len(), 3, "user rules should be left untouched");
        assert_eq!(
            rules[0].as_str().unwrap(),
            "DOMAIN-SUFFIX,example.com,PROXY"
        );
    }

    #[test]
    fn lonely_match_direct_rule_is_replaced() {
        // Some test backends ship `rules: [MATCH,DIRECT]` which would defeat
        // the proxy entirely — auto-shunt should kick in regardless.
        let yaml = r#"
proxy-groups:
  - name: PROXY
    type: select
    proxies: [a]
rules:
  - MATCH,DIRECT
"#;
        let out = patch_mihomo(yaml, "127.0.0.1:9090", "s", 7890, TunnelMode::Tun).unwrap();
        let m = parse(&out);
        let rules = m
            .get(Value::String("rules".into()))
            .unwrap()
            .as_sequence()
            .unwrap();
        assert!(rules.len() > 1, "MATCH,DIRECT should have been replaced");
        assert!(rules
            .iter()
            .any(|r| r.as_str() == Some("GEOSITE,category-ads-all,REJECT")));
    }

    #[test]
    fn non_mapping_root_is_rejected() {
        let err = patch_mihomo("- 1\n- 2\n", "127.0.0.1:9090", "s", 7890, TunnelMode::Tun)
            .unwrap_err();
        match err {
            XboardError::Config(_) => {}
            other => panic!("expected Config error, got {:?}", other),
        }
    }
}
