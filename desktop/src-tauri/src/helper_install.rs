//! macOS one-consent installation of a fixed helper and mihomo snapshot.
//! Elevated code validates open source files and their SHA256, then atomically
//! installs them in root-owned storage. No user-writable script runs as root.
#![cfg(target_os = "macos")]

use async_trait::async_trait;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use tauri::{AppHandle, Manager};
use xboard_core::kernel::launcher::{HelperInstaller, LauncherError};
#[path = "helper_install_script.rs"]
mod script;

pub(crate) const INSTALLED_HELPER_PATH: &str = script::HELPER;
pub(crate) const PLIST_PATH: &str = script::PLIST;

pub fn build_installer(app: &AppHandle) -> Arc<dyn HelperInstaller> {
    Arc::new(OsascriptInstaller {
        helper: bundled_path(app, "xboard-helper"),
        kernel: bundled_path(app, "mihomo"),
    })
}

#[derive(Debug)]
struct OsascriptInstaller {
    helper: Option<PathBuf>,
    kernel: Option<PathBuf>,
}

#[async_trait]
impl HelperInstaller for OsascriptInstaller {
    async fn install(&self) -> Result<(), LauncherError> {
        let helper = self.helper.clone().ok_or_else(|| {
            LauncherError::ServiceMissing("应用中缺少 xboard-helper，请重新安装完整客户端。".into())
        })?;
        let kernel = self.kernel.clone().ok_or_else(|| {
            LauncherError::ServiceMissing("应用中缺少 mihomo，请重新安装完整客户端。".into())
        })?;
        tokio::task::spawn_blocking(move || run_install(&helper, &kernel))
            .await
            .map_err(|error| LauncherError::Other(format!("installer join: {error}")))?
    }
    async fn uninstall(&self) -> Result<(), LauncherError> {
        tokio::task::spawn_blocking(|| run_as_admin(&script::uninstall_script()))
            .await
            .map_err(|error| LauncherError::Other(format!("uninstaller join: {error}")))?
    }
}

fn run_install(helper: &Path, kernel: &Path) -> Result<(), LauncherError> {
    use std::os::unix::fs::DirBuilderExt;
    extern "C" {
        fn geteuid() -> u32;
    }
    let uid = unsafe { geteuid() };
    if uid == 0 {
        return Err(LauncherError::NotPermitted(
            "请以普通用户运行 Sufe，由系统授权安装特权组件。".into(),
        ));
    }
    let unique = std::process::Command::new("/usr/bin/uuidgen")
        .output()
        .map_err(LauncherError::Io)?;
    let unique = String::from_utf8_lossy(&unique.stdout).trim().to_string();
    if unique.len() != 36
        || !unique
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() || byte == b'-')
    {
        return Err(LauncherError::Other("无法创建安装快照标识。".into()));
    }
    let directory = std::env::temp_dir().join(format!("sufe-install-{unique}"));
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&directory)
        .map_err(LauncherError::Io)?;
    let result = (|| {
        let helper_copy = snapshot(helper, &directory.join("helper"))?;
        let kernel_copy = snapshot(kernel, &directory.join("kernel"))?;
        let helper_hash = sha256(&helper_copy)?;
        let kernel_hash = sha256(&kernel_copy)?;
        let command =
            script::install_script(&helper_copy, &kernel_copy, &helper_hash, &kernel_hash, uid)
                .map_err(LauncherError::Other)?;
        run_as_admin(&command)
    })();
    // Only our unique, private directory is removed; no caller-provided path.
    let _ = std::fs::remove_dir_all(&directory);
    result
}

fn snapshot(source: &Path, destination: &Path) -> Result<PathBuf, LauncherError> {
    use std::{
        io::{Read, Write},
        os::unix::fs::OpenOptionsExt,
    };
    // macOS O_NOFOLLOW/O_NONBLOCK values are exposed without adding libc to
    // the GUI dependency graph. The elevated installer checks the snapshot again.
    const O_NOFOLLOW: i32 = 0x00000100;
    const O_NONBLOCK: i32 = 0x00000004;
    let mut input = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(O_NOFOLLOW | O_NONBLOCK)
        .open(source)
        .map_err(LauncherError::Io)?;
    let metadata = input.metadata().map_err(LauncherError::Io)?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > 256 * 1024 * 1024 {
        return Err(LauncherError::Other("安装资源不是有效的普通文件。".into()));
    }
    let mut output = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(O_NOFOLLOW)
        .open(destination)
        .map_err(LauncherError::Io)?;
    let mut remaining = metadata.len();
    let mut buffer = [0; 65536];
    while remaining > 0 {
        let size = input.read(&mut buffer).map_err(LauncherError::Io)?;
        if size == 0 || size as u64 > remaining {
            return Err(LauncherError::Other(
                "安装资源在读取过程中改变，请重试。".into(),
            ));
        }
        output
            .write_all(&buffer[..size])
            .map_err(LauncherError::Io)?;
        remaining -= size as u64;
    }
    if input.read(&mut buffer[..1]).map_err(LauncherError::Io)? != 0 {
        return Err(LauncherError::Other("安装资源大小发生变化。".into()));
    }
    output.sync_all().map_err(LauncherError::Io)?;
    Ok(destination.to_path_buf())
}

fn sha256(path: &Path) -> Result<String, LauncherError> {
    let output = std::process::Command::new("/usr/bin/shasum")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LC_ALL", "C")
        .args(["-a", "256", "--"])
        .arg(path)
        .output()
        .map_err(LauncherError::Io)?;
    let value = String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    if !output.status.success()
        || value.len() != 64
        || !value.bytes().all(|c| c.is_ascii_hexdigit())
    {
        return Err(LauncherError::Other("安装资源 SHA256 校验失败。".into()));
    }
    Ok(value)
}

fn run_as_admin(command: &str) -> Result<(), LauncherError> {
    let apple_script = format!(
        "do shell script \"{}\" with administrator privileges",
        command.replace('\\', "\\\\").replace('"', "\\\"")
    );
    let output = std::process::Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg(apple_script)
        .output()
        .map_err(LauncherError::Io)?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    if stderr.contains("-128") || stderr.to_ascii_lowercase().contains("user canceled") {
        return Err(LauncherError::NeedsConsent("用户取消了管理员授权。".into()));
    }
    Err(LauncherError::Other(format!("特权组件安装失败：{stderr}")))
}

fn bundled_path(app: &AppHandle, name: &str) -> Option<PathBuf> {
    let triple = if cfg!(target_arch = "aarch64") {
        "aarch64-apple-darwin"
    } else {
        "x86_64-apple-darwin"
    };
    let mut roots = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            roots.push(parent.to_path_buf());
        }
    }
    if let Ok(resources) = app.path().resource_dir() {
        roots.push(resources.clone());
        roots.push(resources.join("binaries"));
    }
    #[cfg(debug_assertions)]
    roots.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("binaries"));
    for root in roots {
        for filename in [name.to_string(), format!("{name}-{triple}")] {
            let candidate = root.join(filename);
            if std::fs::symlink_metadata(&candidate)
                .is_ok_and(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
            {
                return Some(candidate);
            }
        }
    }
    None
}
