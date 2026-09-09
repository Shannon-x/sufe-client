//! Runtime YAML contains both subscription credentials and a controller token.
//! Pin directory handles, reject links, and publish only a private regular file.
use std::{
    ffi::{CStr, CString},
    fs::{File, Metadata, Permissions},
    io::{self, Write},
    os::{
        fd::{AsRawFd, FromRawFd, RawFd},
        unix::{ffi::OsStrExt, fs::MetadataExt, fs::PermissionsExt},
    },
    path::{Component, Path},
};

fn denied(reason: &str) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, reason)
}

fn open_at(parent: RawFd, name: &CStr, flags: i32, mode: libc::mode_t) -> io::Result<File> {
    // SAFETY: name is NUL-terminated; a successful descriptor is owned exactly once.
    let fd = unsafe {
        libc::openat(
            parent,
            name.as_ptr(),
            flags | libc::O_CLOEXEC,
            mode as libc::c_uint,
        )
    };
    if fd < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(unsafe { File::from_raw_fd(fd) })
    }
}

fn private_permissions(file: &File, mode: u32) -> io::Result<()> {
    file.set_permissions(Permissions::from_mode(mode))?;
    // macOS extended ACLs can grant access even after chmod(0600/0700).
    // Remove inherited entries through the same descriptor before any secret write.
    #[cfg(target_os = "macos")]
    {
        const ACL_TYPE_EXTENDED: libc::c_int = 0x00000100;
        extern "C" {
            fn acl_init(count: libc::c_int) -> *mut libc::c_void;
            fn acl_set_fd_np(
                fd: libc::c_int,
                acl: *mut libc::c_void,
                kind: libc::c_int,
            ) -> libc::c_int;
            fn acl_free(acl: *mut libc::c_void) -> libc::c_int;
        }
        // SAFETY: initialize an empty ACL, apply it to our open descriptor and
        // free it once; errno is captured before acl_free can change it.
        let acl = unsafe { acl_init(0) };
        if acl.is_null() {
            return Err(io::Error::last_os_error());
        }
        let status = unsafe { acl_set_fd_np(file.as_raw_fd(), acl, ACL_TYPE_EXTENDED) };
        let error = (status != 0).then(io::Error::last_os_error);
        unsafe { acl_free(acl) };
        if let Some(error) = error {
            return Err(error);
        }
    }
    Ok(())
}

fn private_directory(path: &Path) -> io::Result<File> {
    #[cfg(target_os = "macos")]
    let resolved = resolve_macos_system_alias(path)?;
    #[cfg(target_os = "macos")]
    let path = resolved.as_path();
    let components: Vec<_> = path.components().collect();
    if components.len() < 2 || components.first() != Some(&Component::RootDir) {
        return Err(denied("kernel directory must be an absolute non-root path"));
    }
    let uid = unsafe { libc::geteuid() };
    let mut directory = File::open("/")?;
    #[cfg(target_os = "macos")]
    reject_macos_acl_grants(&directory)?;
    for (index, component) in components.iter().enumerate().skip(1) {
        let Component::Normal(name) = component else {
            return Err(denied("kernel directory must not contain parent traversal"));
        };
        let name = CString::new(name.as_bytes())
            .map_err(|_| denied("kernel directory contains a NUL byte"))?;
        let flags = libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW;
        let child = match open_at(directory.as_raw_fd(), &name, flags, 0) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                // SAFETY: parent is a pinned directory and name is one component.
                if unsafe { libc::mkdirat(directory.as_raw_fd(), name.as_ptr(), 0o700) } != 0 {
                    let error = io::Error::last_os_error();
                    if error.kind() != io::ErrorKind::AlreadyExists {
                        return Err(error);
                    }
                }
                open_at(directory.as_raw_fd(), &name, flags, 0)?
            }
            Err(error) => return Err(error),
        };
        let metadata = child.metadata()?;
        let last = index == components.len() - 1;
        if last {
            if metadata.uid() != uid {
                return Err(denied("kernel directory is owned by another user"));
            }
            private_permissions(&child, 0o700)?;
        } else {
            #[cfg(target_os = "macos")]
            reject_macos_acl_grants(&child)?;
            // Permit the root-owned sticky /tmp ancestor used by tests and
            // temporary app data, but not an attacker-writable normal ancestor.
            let sticky_root = metadata.uid() == 0 && metadata.mode() & 0o1000 != 0;
            if (metadata.uid() != uid && metadata.uid() != 0)
                || (metadata.mode() & 0o022 != 0 && !sticky_root)
            {
                return Err(denied("kernel directory has an unsafe ancestor"));
            }
        }
        directory = child;
    }
    Ok(directory)
}

#[cfg(target_os = "macos")]
fn resolve_macos_system_alias(path: &Path) -> io::Result<std::path::PathBuf> {
    for (alias, target) in [("/var", "/private/var"), ("/tmp", "/private/tmp")] {
        if let Ok(suffix) = path.strip_prefix(alias) {
            let metadata = std::fs::symlink_metadata(alias)?;
            if metadata.file_type().is_symlink() {
                let link = std::fs::read_link(alias)?;
                if metadata.uid() != 0 || Path::new("/").join(link) != Path::new(target) {
                    return Err(denied("unrecognized macOS system directory alias"));
                }
                return Ok(Path::new(target).join(suffix));
            }
        }
    }
    Ok(path.to_owned())
}

#[cfg(target_os = "macos")]
fn reject_macos_acl_grants(file: &File) -> io::Result<()> {
    // Apple libc: sys/acl.h and posix1e/acl_entry.c. Unlike the Linux API,
    // successful iteration returns 0 and its end is reported as EINVAL.
    extern "C" {
        fn acl_get_fd_np(fd: libc::c_int, kind: libc::c_int) -> *mut libc::c_void;
        fn acl_valid(acl: *mut libc::c_void) -> libc::c_int;
        fn acl_get_entry(
            acl: *mut libc::c_void,
            index: libc::c_int,
            entry: *mut *mut libc::c_void,
        ) -> libc::c_int;
        fn acl_get_tag_type(entry: *mut libc::c_void, tag: *mut libc::c_int) -> libc::c_int;
        fn acl_free(acl: *mut libc::c_void) -> libc::c_int;
    }
    let acl = unsafe { acl_get_fd_np(file.as_raw_fd(), 0x100) };
    if acl.is_null() {
        let error = io::Error::last_os_error();
        return match error.raw_os_error() {
            Some(libc::ENOENT | libc::ENOATTR) => Ok(()),
            _ => Err(error),
        };
    }
    let result = (|| {
        if unsafe { acl_valid(acl) } != 0 {
            return Err(io::Error::last_os_error());
        }
        for count in 0..=128 {
            let mut entry = std::ptr::null_mut();
            if unsafe { acl_get_entry(acl, if count == 0 { 0 } else { -1 }, &mut entry) } != 0 {
                let error = io::Error::last_os_error();
                return if error.raw_os_error() == Some(libc::EINVAL) {
                    Ok(())
                } else {
                    Err(error)
                };
            }
            let mut tag = 0;
            if count == 128 || unsafe { acl_get_tag_type(entry, &mut tag) } != 0 || tag != 2 {
                return Err(denied(
                    "kernel directory ancestor has a granting or invalid ACL",
                ));
            }
        }
        unreachable!()
    })();
    unsafe { acl_free(acl) };
    result
}

fn validate_existing(metadata: &Metadata) -> io::Result<()> {
    if !metadata.is_file() || metadata.uid() != unsafe { libc::geteuid() } || metadata.nlink() != 1
    {
        return Err(denied(
            "runtime config must be an owned regular file without hard links",
        ));
    }
    Ok(())
}

pub(super) fn write(directory: &Path, contents: &[u8]) -> io::Result<()> {
    let directory = private_directory(directory)?;
    let target = CString::new("config.yaml").unwrap();
    let validate_target = || match open_at(
        directory.as_raw_fd(),
        &target,
        libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK,
        0,
    ) {
        Ok(existing) => validate_existing(&existing.metadata()?),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    };
    validate_target()?;
    let temporary = CString::new(format!(".config-{}.tmp", uuid::Uuid::new_v4())).unwrap();
    let mut file = open_at(
        directory.as_raw_fd(),
        &temporary,
        libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW,
        0o600,
    )?;
    let result = (|| {
        private_permissions(&file, 0o600)?;
        file.write_all(contents)?;
        file.sync_all()?;
        validate_target()?;
        // The leaf directory is now private. Rename replaces only our verified
        // config entry; it never follows a symlink or truncates a linked inode.
        let status = unsafe {
            libc::renameat(
                directory.as_raw_fd(),
                temporary.as_ptr(),
                directory.as_raw_fd(),
                target.as_ptr(),
            )
        };
        if status != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    })();
    if result.is_err() {
        // SAFETY: cleanup only the unique name that this call created.
        unsafe { libc::unlinkat(directory.as_raw_fd(), temporary.as_ptr(), 0) };
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::symlink, path::PathBuf};

    struct Scratch(PathBuf);
    impl Scratch {
        fn new() -> Self {
            // /var on macOS is a system symlink; use the resolved test root so
            // the code under test can continue to reject all symlink traversal.
            let path = std::env::temp_dir()
                .canonicalize()
                .unwrap()
                .join(format!("sufe-private-config-{}", uuid::Uuid::new_v4()));
            fs::create_dir(&path).unwrap();
            fs::set_permissions(&path, Permissions::from_mode(0o700)).unwrap();
            Self(path)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[test]
    fn creates_private_config_and_tightens_legacy_permissions() {
        let scratch = Scratch::new();
        let directory = scratch.0.join("kernel");
        write(&directory, b"secret: first").unwrap();
        let config = directory.join("config.yaml");
        assert_eq!(fs::metadata(&directory).unwrap().mode() & 0o777, 0o700);
        assert_eq!(fs::metadata(&config).unwrap().mode() & 0o777, 0o600);
        fs::set_permissions(&directory, Permissions::from_mode(0o755)).unwrap();
        fs::set_permissions(&config, Permissions::from_mode(0o644)).unwrap();
        write(&directory, b"secret: replacement").unwrap();
        assert_eq!(fs::metadata(&directory).unwrap().mode() & 0o777, 0o700);
        assert_eq!(fs::metadata(&config).unwrap().mode() & 0o777, 0o600);
        assert_eq!(fs::read(config).unwrap(), b"secret: replacement");
        assert_eq!(fs::read_dir(directory).unwrap().count(), 1);
    }

    #[test]
    fn rejects_config_symlink_without_changing_its_target() {
        let scratch = Scratch::new();
        let directory = scratch.0.join("kernel");
        fs::create_dir(&directory).unwrap();
        let other = scratch.0.join("unrelated");
        fs::write(&other, b"keep me").unwrap();
        symlink(&other, directory.join("config.yaml")).unwrap();
        assert!(write(&directory, b"secret: no").is_err());
        assert_eq!(fs::read(other).unwrap(), b"keep me");
        assert_eq!(fs::read_dir(directory).unwrap().count(), 1);
    }

    #[test]
    fn rejects_hard_link_without_changing_the_other_file() {
        let scratch = Scratch::new();
        let directory = scratch.0.join("kernel");
        fs::create_dir(&directory).unwrap();
        let other = scratch.0.join("unrelated");
        fs::write(&other, b"keep me").unwrap();
        fs::hard_link(&other, directory.join("config.yaml")).unwrap();
        assert!(write(&directory, b"secret: no").is_err());
        assert_eq!(fs::read(other).unwrap(), b"keep me");
    }

    #[test]
    fn rejects_directory_symlink_and_writable_ancestor() {
        let scratch = Scratch::new();
        let actual = scratch.0.join("actual");
        fs::create_dir(&actual).unwrap();
        let linked = scratch.0.join("linked");
        symlink(&actual, &linked).unwrap();
        assert!(write(&linked, b"secret: no").is_err());
        assert!(write(&linked.join("kernel"), b"secret: no").is_err());
        assert!(!actual.join("config.yaml").exists());
        fs::set_permissions(&actual, Permissions::from_mode(0o777)).unwrap();
        assert!(write(&actual.join("kernel"), b"secret: no").is_err());
        assert!(!actual.join("kernel").exists());
    }

    #[test]
    fn rejects_directory_and_fifo_at_config_path() {
        let scratch = Scratch::new();
        let directory = scratch.0.join("kernel");
        let config = directory.join("config.yaml");
        fs::create_dir_all(&config).unwrap();
        assert!(write(&directory, b"secret: no").is_err());
        fs::remove_dir(&config).unwrap();
        let path = CString::new(config.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
        assert!(write(&directory, b"secret: no").is_err());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn removes_macos_extended_acl_from_runtime_directory() {
        let scratch = Scratch::new();
        let directory = scratch.0.join("kernel");
        fs::create_dir(&directory).unwrap();
        assert!(std::process::Command::new("/bin/chmod")
            .args([
                "+a",
                "everyone allow list,search,readattr,readextattr,readsecurity"
            ])
            .arg(&directory)
            .status()
            .unwrap()
            .success());
        write(&directory, b"secret: private").unwrap();
        for path in [&directory, &directory.join("config.yaml")] {
            let listing = std::process::Command::new("/bin/ls")
                .arg("-lde")
                .arg(path)
                .output()
                .unwrap();
            assert!(listing.status.success());
            assert!(!String::from_utf8_lossy(&listing.stdout)
                .lines()
                .any(|line| line.trim_start().starts_with("0:")));
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn rejects_granting_ancestor_acl_but_accepts_protective_deny() {
        let scratch = Scratch::new();
        let directory = scratch.0.join("kernel");
        let chmod = |ace: &str| {
            assert!(std::process::Command::new("/bin/chmod")
                .args(["+a", ace])
                .arg(&scratch.0)
                .status()
                .unwrap()
                .success());
        };
        chmod("everyone deny delete");
        write(&directory, b"secret: safe").unwrap();
        chmod("everyone allow add_file,add_subdirectory,delete_child");
        assert!(write(&directory, b"secret: denied").is_err());
        assert_eq!(
            fs::read(directory.join("config.yaml")).unwrap(),
            b"secret: safe"
        );
        assert!(std::process::Command::new("/bin/chmod")
            .arg("-N")
            .arg(&scratch.0)
            .status()
            .unwrap()
            .success());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn accepts_only_known_macos_system_aliases() {
        assert_eq!(
            resolve_macos_system_alias(Path::new("/var/tmp/kernel")).unwrap(),
            Path::new("/private/var/tmp/kernel")
        );
        assert_eq!(
            resolve_macos_system_alias(Path::new("/tmp/kernel")).unwrap(),
            Path::new("/private/tmp/kernel")
        );
    }
}
