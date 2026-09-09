//! Keep AppImage itself unprivileged; only a verified, protected kernel gets
//! Linux network capabilities after one explicit desktop authorization.
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::broadcast;
use xboard_core::kernel::{
    linux_caps, DirectLauncher, KernelFailure, KernelLauncher, KernelSpawnSpec, LaunchHandle,
    LauncherError,
};

pub const MANAGED_KERNEL: &str = "/usr/local/lib/sufe/mihomo";
const KERNEL_SHA256: &str = env!("SUFE_LINUX_KERNEL_SHA256");
const INSTALL_SCRIPT: &str = include_str!("../build_extras/install-linux-kernel.sh");

fn trusted_file(path: &Path) -> bool {
    let Ok(file) = std::fs::symlink_metadata(path) else {
        return false;
    };
    if !file.is_file() {
        return false;
    }
    let mut current = Some(path);
    while let Some(component) = current {
        let Ok(meta) = std::fs::symlink_metadata(component) else {
            return false;
        };
        if meta.file_type().is_symlink() || meta.uid() != 0 || meta.mode() & 0o022 != 0 {
            return false;
        }
        current = component.parent();
    }
    true
}

fn verified_hash(path: &Path) -> bool {
    let Ok(output) = Command::new("/usr/bin/sha256sum").arg(path).output() else {
        return false;
    };
    output.status.success()
        && String::from_utf8_lossy(&output.stdout)
            .split_whitespace()
            .next()
            == Some(KERNEL_SHA256)
}

fn ready(path: &Path) -> bool {
    trusted_file(path) && linux_caps::has_file_capability(path) && verified_hash(path)
}

/// Read-only health query. The caller runs filesystem/hash checks off the UI thread.
pub fn ready_kernel(bundled: &Path) -> Option<PathBuf> {
    if ready(bundled) {
        return Some(bundled.to_path_buf());
    }
    let managed = PathBuf::from(MANAGED_KERNEL);
    ready(&managed).then_some(managed)
}

fn authorization_result(code: Option<i32>, detail: &str) -> Result<(), LauncherError> {
    match code {
        Some(0) => Ok(()),
        Some(126) => Err(LauncherError::NeedsConsent("已取消 TUN 安装授权".into())),
        Some(127) => Err(LauncherError::NotPermitted(
            "无法取得系统授权，请确认已安装 polkit 并在桌面会话中运行".into(),
        )),
        _ => Err(LauncherError::Other(format!(
            "TUN 内核安装失败：{}",
            detail.trim()
        ))),
    }
}

async fn interfaces(arguments: &[&str]) -> Result<Vec<serde_json::Value>, LauncherError> {
    let ip = if Path::new("/usr/sbin/ip").is_file() {
        "/usr/sbin/ip"
    } else {
        "/usr/bin/ip"
    };
    let output = tokio::time::timeout(
        Duration::from_secs(2),
        tokio::process::Command::new(ip)
            .arg("-j")
            .args(arguments)
            .stdin(Stdio::null())
            .output(),
    )
    .await
    .map_err(|_| LauncherError::Other("读取 TUN 网卡状态超时".into()))??;
    if !output.status.success() {
        return Err(LauncherError::Other(
            "无法检查 TUN 网卡，请确认 iproute2 已安装".into(),
        ));
    }
    serde_json::from_slice(&output.stdout)
        .map_err(|e| LauncherError::Other(format!("网卡状态无效：{e}")))
}

fn device_has_ipv4(device: &str, entries: &[serde_json::Value]) -> bool {
    entries.iter().any(|entry| {
        entry.get("ifname").and_then(|v| v.as_str()) == Some(device)
            && entry
                .get("flags")
                .and_then(|v| v.as_array())
                .is_some_and(|flags| flags.iter().any(|v| v.as_str() == Some("UP")))
            && entry
                .get("addr_info")
                .and_then(|v| v.as_array())
                .is_some_and(|addresses| {
                    addresses.iter().any(|address| {
                        address.get("family").and_then(|v| v.as_str()) == Some("inet")
                            && address
                                .get("local")
                                .and_then(|v| v.as_str())
                                .and_then(|v| v.parse::<std::net::Ipv4Addr>().ok())
                                .is_some_and(|ip| !ip.is_unspecified() && !ip.is_loopback())
                    })
                })
    })
}

#[derive(Debug)]
pub struct LinuxManagedLauncher {
    bundled: PathBuf,
    direct: DirectLauncher,
    tun_ready: AtomicBool,
}

impl LinuxManagedLauncher {
    pub fn new(bundled: PathBuf) -> Self {
        Self {
            bundled,
            direct: DirectLauncher::new(),
            tun_ready: AtomicBool::new(false),
        }
    }
}

#[async_trait]
impl KernelLauncher for LinuxManagedLauncher {
    async fn ensure_privileged(&self) -> Result<(), LauncherError> {
        self.tun_ready.store(false, Ordering::SeqCst);
        let bundled = self.bundled.clone();
        if tokio::task::spawn_blocking(move || ready_kernel(&bundled))
            .await
            .map_err(|e| LauncherError::Other(e.to_string()))?
            .is_some()
        {
            self.tun_ready.store(true, Ordering::SeqCst);
            return Ok(());
        }
        let source = self.bundled.canonicalize()?;
        let candidate = source.clone();
        if !tokio::task::spawn_blocking(move || verified_hash(&candidate))
            .await
            .map_err(|e| LauncherError::Other(e.to_string()))?
        {
            return Err(LauncherError::NotPermitted(
                "随包内核校验失败，请重新安装客户端".into(),
            ));
        }
        let mut command = tokio::process::Command::new("/usr/bin/pkexec");
        command
            .arg("--disable-internal-agent")
            .arg("/bin/sh")
            .arg("-c")
            .arg(INSTALL_SCRIPT.replace("@SUFE_KERNEL_SHA256@", KERNEL_SHA256))
            .arg("sufe-install-kernel")
            .arg(&source)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let output = tokio::time::timeout(Duration::from_secs(120), command.output())
            .await
            .map_err(|_| LauncherError::NeedsConsent("TUN 安装授权超时，请重试".into()))?
            .map_err(|e| {
                LauncherError::NotPermitted(format!(
                    "无法启动 polkit 授权，请安装 polkit 或使用 deb 安装包：{e}"
                ))
            })?;
        authorization_result(
            output.status.code(),
            &String::from_utf8_lossy(&output.stderr),
        )?;
        let managed = PathBuf::from(MANAGED_KERNEL);
        if !tokio::task::spawn_blocking(move || ready(&managed))
            .await
            .map_err(|e| LauncherError::Other(e.to_string()))?
        {
            return Err(LauncherError::NotPermitted(
                "TUN 内核安装后的权限或哈希检查失败".into(),
            ));
        }
        self.tun_ready.store(true, Ordering::SeqCst);
        Ok(())
    }

    async fn spawn(&self, mut spec: KernelSpawnSpec) -> Result<LaunchHandle, LauncherError> {
        // Read the actual generated profile so a failed TUN attempt followed by
        // system-proxy connect cannot reuse a stale authorization flag.
        let text = tokio::fs::read_to_string(&spec.cfg_path).await?;
        let config: serde_yaml::Value = serde_yaml::from_str(&text)
            .map_err(|e| LauncherError::Other(format!("解析连接配置失败：{e}")))?;
        let tun = config
            .get("tun")
            .and_then(|v| v.get("enable"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let device = if tun {
            let name = config
                .get("tun")
                .and_then(|v| v.get("device"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if name.is_empty()
                || name.len() > 15
                || !name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
            {
                return Err(LauncherError::Other(
                    "TUN 配置缺少有效的固定网卡名称".into(),
                ));
            }
            if interfaces(&["link", "show"])
                .await?
                .iter()
                .any(|entry| entry.get("ifname").and_then(|v| v.as_str()) == Some(name))
            {
                return Err(LauncherError::NotPermitted(format!(
                    "网卡 {name} 已存在，请先关闭占用它的 VPN；客户端不会操作其他进程的网卡"
                )));
            }
            Some(name.to_owned())
        } else {
            None
        };
        if tun {
            if !self.tun_ready.load(Ordering::SeqCst) {
                return Err(LauncherError::NotPermitted("请先完成 TUN 授权".into()));
            }
            let bundled = self.bundled.clone();
            spec.exec_path = tokio::task::spawn_blocking(move || ready_kernel(&bundled))
                .await
                .map_err(|e| LauncherError::Other(e.to_string()))?
                .ok_or_else(|| {
                    LauncherError::NotPermitted("TUN 内核已变化，请重新连接并授权".into())
                })?;
        } else {
            self.tun_ready.store(false, Ordering::SeqCst);
            spec.exec_path = self.bundled.clone();
        }
        let controller_url = format!("http://{}/version", spec.controller_addr);
        let controller_secret = spec.controller_secret.clone();
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(2))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| LauncherError::Other(e.to_string()))?;
        let handle = self.direct.spawn(spec).await?;
        if let Some(device) = device {
            let readiness = async {
                let deadline = tokio::time::Instant::now() + Duration::from_secs(12);
                while tokio::time::Instant::now() < deadline {
                    let entries = interfaces(&["address", "show"]).await?;
                    if device_has_ipv4(&device, &entries) {
                        if let Ok(response) = client.get(&controller_url).bearer_auth(&controller_secret).send().await {
                            if response.status().is_success() {
                                if let Ok(version) = response.json::<serde_json::Value>().await {
                                    if version.get("version").and_then(|v| v.as_str()).is_some() { return Ok(()); }
                                }
                            }
                        }
                    }
                    tokio::time::sleep(Duration::from_millis(250)).await;
                }
                Err(LauncherError::Other(format!("内核控制器已启动，但 TUN 网卡 {device} 未就绪（需要 UP 和 IPv4）。请检查系统权限与内核日志")))
            }.await;
            if let Err(error) = readiness {
                self.tun_ready.store(false, Ordering::SeqCst);
                self.direct.stop(handle).await?;
                return Err(error);
            }
        }
        Ok(handle)
    }

    async fn stop(&self, handle: LaunchHandle) -> Result<(), LauncherError> {
        self.tun_ready.store(false, Ordering::SeqCst);
        self.direct.stop(handle).await
    }

    fn name(&self) -> &'static str {
        "linux-managed"
    }
    fn failure_stream(&self) -> Option<broadcast::Receiver<KernelFailure>> {
        self.direct.failure_stream()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancelled_and_unavailable_authorizations_are_not_success() {
        assert!(matches!(
            authorization_result(Some(126), ""),
            Err(LauncherError::NeedsConsent(_))
        ));
        assert!(matches!(
            authorization_result(Some(127), ""),
            Err(LauncherError::NotPermitted(_))
        ));
        assert!(authorization_result(Some(1), "checksum mismatch").is_err());
        assert!(authorization_result(Some(0), "").is_ok());
    }
    #[test]
    fn missing_managed_kernel_is_not_ready() {
        assert!(!ready(Path::new("/nonexistent/sufe-test-mihomo")));
    }
    #[test]
    fn controller_without_an_up_addressed_tun_is_not_ready() {
        use serde_json::json;
        assert!(!device_has_ipv4("Mihomo", &[]));
        assert!(!device_has_ipv4(
            "Mihomo",
            &[
                json!({"ifname":"Mihomo","flags":[],"addr_info":[{"family":"inet","local":"198.18.0.1"}]})
            ]
        ));
        assert!(!device_has_ipv4(
            "Mihomo",
            &[json!({"ifname":"Mihomo","flags":["UP"],"addr_info":[]})]
        ));
        assert!(!device_has_ipv4(
            "Mihomo",
            &[
                json!({"ifname":"OtherVPN","flags":["UP"],"addr_info":[{"family":"inet","local":"198.18.0.1"}]})
            ]
        ));
        assert!(device_has_ipv4(
            "Mihomo",
            &[
                json!({"ifname":"Mihomo","flags":["UP"],"addr_info":[{"family":"inet","local":"198.18.0.1"}]})
            ]
        ));
    }
}
