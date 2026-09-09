//! Narrow, validated user routing rules. Never accepts arbitrary YAML or a
//! catch-all rule that could silently replace the provider's routing policy.
use std::{collections::HashSet, net::IpAddr};
use serde::{Deserialize, Serialize};
use crate::error::{Result, XboardError};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CustomRule {
    pub id: String,
    pub kind: String,
    pub value: String,
    pub target: String,
    pub enabled: bool,
}

pub fn validate_rules(rules: &[CustomRule]) -> Result<()> {
    if rules.len() > 200 { return Err(XboardError::Config("最多可添加 200 条自定义规则".into())); }
    let mut ids = HashSet::new();
    for r in rules {
        if r.id.is_empty() || r.id.len() > 64 || !ids.insert(&r.id) {
            return Err(XboardError::Config("规则标识不能为空或重复".into()));
        }
        if !matches!(r.target.as_str(), "DIRECT" | "PROXY" | "REJECT") {
            return Err(XboardError::Config("规则出口必须为 DIRECT、PROXY 或 REJECT".into()));
        }
        if r.value.is_empty() || r.value.len() > 253 || r.value.trim() != r.value {
            return Err(XboardError::Config("规则内容为空、过长或含首尾空格".into()));
        }
        let valid = match r.kind.as_str() {
            "DOMAIN" | "DOMAIN-SUFFIX" => r.value.split('.').all(|label| {
                !label.is_empty() && label.len() <= 63 && !label.starts_with('-') && !label.ends_with('-')
                    && label.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            }),
            "DOMAIN-KEYWORD" => r.value.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_')),
            "IP-CIDR" | "IP-CIDR6" => r.value.split_once('/').is_some_and(|(ip, prefix)| {
                match (ip.parse::<IpAddr>(), prefix.parse::<u8>()) {
                    (Ok(IpAddr::V4(_)), Ok(p)) => r.kind == "IP-CIDR" && p > 0 && p <= 32,
                    (Ok(IpAddr::V6(_)), Ok(p)) => r.kind == "IP-CIDR6" && p > 0 && p <= 128,
                    _ => false,
                }
            }),
            _ => false,
        };
        if !valid { return Err(XboardError::Config(format!("无效的 {} 规则：{}", r.kind, r.value))); }
    }
    Ok(())
}

/// Prepend user rules after the controlled subscription patch. PROXY resolves
/// to an actual selectable group; a missing group fails closed.
pub fn apply_custom_rules(yaml: &str, rules: &[CustomRule]) -> Result<String> {
    validate_rules(rules)?;
    if !rules.iter().any(|r| r.enabled) { return Ok(yaml.to_owned()); }
    let mut root: serde_yaml::Value = serde_yaml::from_str(yaml)?;
    let mapping = root.as_mapping_mut().ok_or_else(|| XboardError::Config("订阅必须为 YAML 对象".into()))?;
    let groups_key = serde_yaml::Value::String("proxy-groups".into());
    let group = mapping.get(&groups_key).and_then(|v| v.as_sequence())
        .and_then(|groups| groups.iter().find(|g| g["type"].as_str() == Some("select")).or_else(|| groups.first()))
        .and_then(|g| g["name"].as_str()).map(str::to_owned);
    let mut head = Vec::new();
    for r in rules.iter().filter(|r| r.enabled) {
        let target = if r.target == "PROXY" {
            group.as_deref().ok_or_else(|| XboardError::Config("订阅缺少可用代理组".into()))?
        } else { &r.target };
        if target.contains([',', '\n', '\r']) { return Err(XboardError::Config("订阅代理组名称无效".into())); }
        let extra = if r.kind.starts_with("IP-CIDR") { ",no-resolve" } else { "" };
        head.push(serde_yaml::Value::String(format!("{},{},{}{}", r.kind, r.value, target, extra)));
    }
    let rules_key = serde_yaml::Value::String("rules".into());
    if let Some(existing) = mapping.get(&rules_key).and_then(|v| v.as_sequence()) { head.extend(existing.iter().cloned()); }
    mapping.insert(rules_key, serde_yaml::Value::Sequence(head));
    Ok(serde_yaml::to_string(&root)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn rule(value: &str) -> CustomRule { CustomRule { id: "one".into(), kind: "DOMAIN-SUFFIX".into(), value: value.into(), target: "PROXY".into(), enabled: true } }
    #[test] fn rejects_rule_injection() {
        for value in ["example.com,DIRECT", "example.com\nMATCH,DIRECT", "https://example.com", "*.com", "-bad.com", ""] {
            assert!(validate_rules(&[rule(value)]).is_err(), "{value}");
        }
    }
    #[test] fn validates_cidr_family_and_prefix() {
        let mut r = rule("10.0.0.0/8"); r.kind = "IP-CIDR".into();
        assert!(validate_rules(&[r.clone()]).is_ok());
        r.value = "::1/128".into(); assert!(validate_rules(&[r.clone()]).is_err());
        r.kind = "IP-CIDR6".into(); assert!(validate_rules(&[r.clone()]).is_ok());
        r.value = "::/0".into(); assert!(validate_rules(&[r]).is_err());
    }
    #[test] fn prepends_without_losing_provider_policy() {
        let output = apply_custom_rules("proxy-groups:\n  - name: 专线\n    type: select\nrules:\n  - MATCH,专线\n", &[rule("example.com")]).unwrap();
        let v: serde_yaml::Value = serde_yaml::from_str(&output).unwrap();
        assert_eq!(v["rules"][0].as_str(), Some("DOMAIN-SUFFIX,example.com,专线"));
        assert_eq!(v["rules"][1].as_str(), Some("MATCH,专线"));
    }
    #[test] fn proxy_requires_real_group() { assert!(apply_custom_rules("rules: []", &[rule("example.com")]).is_err()); }
    #[test] fn rejects_duplicate_ids() { assert!(validate_rules(&[rule("a.com"), rule("b.com")]).is_err()); }
}
