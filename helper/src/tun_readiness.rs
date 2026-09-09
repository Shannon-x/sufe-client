//! Host-independent parsing and bounded failure reporting for macOS TUN startup.
use std::{
    io::{Read, Seek, SeekFrom},
    net::Ipv4Addr,
    path::Path,
};

pub fn ready(output: &str) -> bool {
    let mut lines = output.lines();
    let Some(header) = lines.next() else {
        return false;
    };
    if !header.starts_with("utun1989:") {
        return false;
    }
    let flags = header
        .split_once('<')
        .and_then(|(_, rest)| rest.split_once('>'))
        .map(|(flags, _)| flags);
    if !flags.is_some_and(|flags| flags.split(',').any(|flag| flag == "UP")) {
        return false;
    }
    lines.any(|line| {
        let mut fields = line.split_whitespace();
        fields.next() == Some("inet")
            && fields
                .next()
                .and_then(|value| value.parse::<Ipv4Addr>().ok())
                .is_some_and(|address| {
                    !address.is_unspecified() && !address.is_loopback() && !address.is_multicast()
                })
    })
}

pub fn diagnostic(log: &Path, reason: &str, secrets: &[&str]) -> String {
    // The caller created this path under a root-only, random session directory.
    // Read only its tail; discard a truncated first line before redacting so a
    // credential split across the read boundary can never be returned partially.
    let tail = (|| -> std::io::Result<String> {
        let mut file = std::fs::File::open(log)?;
        let size = file.metadata()?.len();
        let offset = size.saturating_sub(16 * 1024);
        file.seek(SeekFrom::Start(offset))?;
        let mut bytes = Vec::new();
        file.take(16 * 1024).read_to_end(&mut bytes)?;
        let mut text = String::from_utf8_lossy(&bytes).into_owned();
        if offset > 0 {
            text = text
                .split_once('\n')
                .map(|(_, rest)| rest.to_owned())
                .unwrap_or_default();
        }
        Ok(text)
    })()
    .unwrap_or_else(|_| "Kernel log unavailable.".into());
    let mut result = format!("{reason}\n{tail}");
    for secret in secrets.iter().filter(|secret| !secret.is_empty()) {
        result = result.replace(secret, "[REDACTED]");
    }
    result = result
        .chars()
        .filter(|ch| !ch.is_control() || matches!(ch, '\n' | '\t'))
        .collect();
    let chars: Vec<char> = result.chars().collect();
    if chars.len() > 8192 {
        format!(
            "{}\n[log truncated]\n{}",
            chars[..1024].iter().collect::<String>(),
            chars[chars.len() - 6144..].iter().collect::<String>()
        )
    } else {
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn requires_exact_interface_up_and_valid_ipv4() {
        let output = "utun1989: flags=8051<UP,POINTOPOINT,RUNNING,MULTICAST> mtu 1500\n\tinet 198.18.77.1 --> 198.18.77.1 netmask 0xfffffffc\n";
        assert!(ready(output));
        for wrong in [
            output.replace("utun1989", "utun7"),
            output.replace("UP,", ""),
            output.replace("inet ", "inet6 "),
            output.replace("198.18.77.1", "0.0.0.0"),
            output.replace("198.18.77.1", "127.0.0.1"),
        ] {
            assert!(!ready(&wrong));
        }
    }
    #[test]
    fn failure_diagnostics_redact_even_when_log_is_unavailable() {
        let text = diagnostic(
            Path::new("/nonexistent-sufe-log-for-test"),
            "private-token failed",
            &["private-token"],
        );
        assert!(!text.contains("private-token"));
        assert!(text.contains("[REDACTED] failed"));
    }
    #[test]
    fn log_tail_is_bounded_and_redacted() {
        let path = std::env::temp_dir().join(format!(
            "sufe-tun-diagnostic-{}-test.log",
            std::process::id()
        ));
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        use std::io::Write;
        file.write_all(
            format!(
                "{}\nTUN startup failed: private-token\n",
                "prefix\n".repeat(4000)
            )
            .as_bytes(),
        )
        .unwrap();
        drop(file);
        let text = diagnostic(&path, "start failed", &["private-token"]);
        std::fs::remove_file(path).unwrap();
        assert!(text.len() < 9000);
        assert!(!text.contains("private-token"));
        assert!(text.contains("TUN startup failed: [REDACTED]"));
    }
}
