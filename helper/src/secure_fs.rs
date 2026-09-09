//! Privileged filesystem operations have no caller-supplied paths.
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    path::Path,
};

pub fn validate_root_chain(path: &Path) -> anyhow::Result<()> {
    if !path.is_absolute()
        || path.components().any(|part| {
            matches!(
                part,
                std::path::Component::ParentDir | std::path::Component::CurDir
            )
        })
    {
        anyhow::bail!("invalid service-owned path");
    }
    let mut current = std::path::PathBuf::new();
    for part in path.components() {
        current.push(part);
        let metadata = std::fs::symlink_metadata(&current)?;
        if metadata.uid() != 0 || metadata.mode() & 0o022 != 0 || metadata.file_type().is_symlink()
        {
            anyhow::bail!("service installation must be root-owned without writable ancestors");
        }
        reject_acl(&current)?;
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn reject_acl(path: &Path) -> anyhow::Result<()> {
    // ACLs can grant writes despite safe POSIX mode bits. System directories
    // may also carry protective deny ACEs: accept only the shared, strict deny
    // policy and continue rejecting every grant or unrecognized listing.
    let result = std::process::Command::new("/bin/ls")
        .env_clear()
        .env("LC_ALL", "C")
        .arg("-lde")
        .arg(path)
        .output()?;
    if !result.status.success() {
        anyhow::bail!("cannot inspect installation ACL");
    }
    let listing = String::from_utf8(result.stdout)?;
    if !crate::acl_policy::verify_listing(listing.as_bytes())? {
        anyhow::bail!(
            "service path has a granting or unrecognized ACL; reinstall into protected storage"
        );
    }
    Ok(())
}
#[cfg(not(target_os = "macos"))]
fn reject_acl(_: &Path) -> anyhow::Result<()> {
    Ok(())
}

pub fn validate_root_file(path: &Path, executable: bool) -> anyhow::Result<()> {
    validate_root_chain(path)?;
    let meta = std::fs::symlink_metadata(path)?;
    if !meta.is_file() || meta.nlink() != 1 || (executable && meta.mode() & 0o111 == 0) {
        anyhow::bail!("invalid installed service file");
    }
    if !executable && meta.mode() & 0o077 != 0 {
        anyhow::bail!("private service file is readable by another user");
    }
    Ok(())
}

pub fn private_directory(path: &Path) -> anyhow::Result<()> {
    directory(path, 0o700)
}
pub fn public_directory(path: &Path) -> anyhow::Result<()> {
    directory(path, 0o755)
}
fn directory(path: &Path, mode: u32) -> anyhow::Result<()> {
    validate_root_chain(
        path.parent()
            .ok_or_else(|| anyhow::anyhow!("invalid service directory"))?,
    )?;
    match std::fs::DirBuilder::new().mode(mode).create(path) {
        Ok(()) => {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.into()),
    }
    validate_root_chain(path)?;
    let meta = std::fs::symlink_metadata(path)?;
    if !meta.is_dir() || meta.mode() & 0o777 != mode {
        anyhow::bail!("service directory permissions differ from expected mode");
    }
    Ok(())
}

pub fn create_private_file(path: &Path) -> anyhow::Result<File> {
    validate_root_chain(
        path.parent()
            .ok_or_else(|| anyhow::anyhow!("missing service parent"))?,
    )?;
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?;
    if file.metadata()?.uid() != 0 {
        anyhow::bail!("service file not root-owned");
    }
    Ok(file)
}

pub fn stage_config(directory: &Path, contents: &[u8]) -> anyhow::Result<std::path::PathBuf> {
    if contents.len() > 2 * 1024 * 1024 {
        anyhow::bail!("configuration exceeds snapshot size limit");
    }
    let path = directory.join("config.yaml");
    let mut file = create_private_file(&path)?;
    file.write_all(contents)?;
    file.sync_all()?;
    Ok(path)
}

pub fn read_owner(path: &Path) -> anyhow::Result<u32> {
    validate_root_file(path, false)?;
    let mut value = String::new();
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?
        .take(32)
        .read_to_string(&mut value)?;
    parse_owner(&value)
}
fn parse_owner(value: &str) -> anyhow::Result<u32> {
    let stripped = value.trim_end_matches('\n');
    if stripped.is_empty() || !stripped.bytes().all(|byte| byte.is_ascii_digit()) {
        anyhow::bail!("invalid installation owner");
    }
    let uid: u32 = stripped.parse()?;
    if uid == 0 || uid == u32::MAX {
        anyhow::bail!("installation owner must be a normal user");
    }
    Ok(uid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    #[test]
    fn owner_is_a_single_non_root_numeric_uid() {
        assert_eq!(parse_owner("501\n").unwrap(), 501);
        for invalid in [
            "",
            "0",
            "-1",
            "501\n502",
            "501 extra",
            "4294967295",
            "999999999999999999",
        ] {
            assert!(parse_owner(invalid).is_err());
        }
    }
    #[test]
    fn rejects_relative_and_traversing_service_paths() {
        assert!(validate_root_chain(Path::new("relative/config.yaml")).is_err());
        assert!(validate_root_chain(Path::new("/Library/../etc/passwd")).is_err());
    }
    #[test]
    fn rejects_user_writable_ancestors_and_symlinks() {
        let root =
            std::env::temp_dir().join(format!("sufe-helper-boundary-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o777)).unwrap();
        let link = root.join("alias");
        std::os::unix::fs::symlink("/", &link).unwrap();
        assert!(validate_root_chain(&root).is_err());
        assert!(validate_root_chain(&link).is_err());
        std::fs::remove_file(link).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
}
