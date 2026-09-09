//! Windows privilege boundary. No paths in this module come from IPC.
use anyhow::{bail, Context, Result};
use std::{
    ffi::OsStr,
    fs::File,
    os::windows::{ffi::OsStrExt, io::FromRawHandle},
    path::{Component, Path, PathBuf},
};
use windows_sys::{
    core::GUID,
    Win32::{
        Foundation::{CloseHandle, LocalFree, ERROR_ALREADY_EXISTS, HANDLE, INVALID_HANDLE_VALUE},
        Security::{Authorization::*, *},
        Storage::FileSystem::*,
        System::{Com::CoTaskMemFree, Pipes::ImpersonateNamedPipeClient, Threading::*},
        UI::Shell::{FOLDERID_ProgramData, FOLDERID_ProgramFiles, SHGetKnownFolderPath},
    },
};

const TRUSTED_INSTALLER: &str = "S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464";
const PRIVATE_SDDL: &str = "O:BAG:BAD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)";
const WRITE_MASK: u32 =
    0x10000 | 0x40000 | 0x80000 | 0x40000000 | 0x10000000 | 0x2 | 0x4 | 0x10 | 0x40 | 0x100;
const ANCESTOR_MASK: u32 = 0x10000 | 0x40000 | 0x80000 | 0x40000000 | 0x10000000 | 0x40;

pub fn wide(value: impl AsRef<OsStr>) -> Vec<u16> {
    value.as_ref().encode_wide().chain(Some(0)).collect()
}

pub struct SecurityAttributes {
    pub attrs: SECURITY_ATTRIBUTES,
    descriptor: PSECURITY_DESCRIPTOR,
}
impl Drop for SecurityAttributes {
    fn drop(&mut self) {
        unsafe {
            LocalFree(self.descriptor);
        }
    }
}
impl SecurityAttributes {
    pub fn from_sddl(sddl: &str) -> Result<Self> {
        let mut descriptor = std::ptr::null_mut();
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                wide(sddl).as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                std::ptr::null_mut(),
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(Self {
            attrs: SECURITY_ATTRIBUTES {
                nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: descriptor,
                bInheritHandle: 0,
            },
            descriptor,
        })
    }
}

pub fn validate_sid(sid: &str) -> Result<()> {
    if !sid.starts_with("S-")
        || sid.len() > 184
        || !sid
            .bytes()
            .all(|b| b.is_ascii_digit() || b == b'-' || b == b'S')
    {
        bail!("invalid Windows user SID");
    }
    let mut value = std::ptr::null_mut();
    if unsafe { ConvertStringSidToSidW(wide(sid).as_ptr(), &mut value) } == 0 {
        bail!("invalid Windows user SID");
    }
    let canonical = sid_string(value);
    unsafe {
        LocalFree(value);
    }
    if canonical? != sid {
        bail!("non-canonical Windows SID");
    }
    Ok(())
}

fn sid_string(sid: PSID) -> Result<String> {
    if sid.is_null() || unsafe { IsValidSid(sid) } == 0 {
        bail!("invalid SID in security descriptor");
    }
    let mut pointer = std::ptr::null_mut();
    if unsafe { ConvertSidToStringSidW(sid, &mut pointer) } == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let mut length = 0;
    unsafe {
        while *pointer.add(length) != 0 {
            length += 1;
        }
    }
    let result = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(pointer, length) });
    unsafe {
        LocalFree(pointer as _);
    }
    Ok(result)
}

fn token_sid(token: HANDLE) -> Result<String> {
    let mut required = 0;
    unsafe {
        GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut required);
    }
    if required == 0 {
        bail!("TokenUser size query failed");
    }
    // usize storage gives TOKEN_USER the native pointer alignment it requires.
    let mut buffer = vec![0usize; (required as usize).div_ceil(std::mem::size_of::<usize>())];
    if unsafe {
        GetTokenInformation(
            token,
            TokenUser,
            buffer.as_mut_ptr() as _,
            required,
            &mut required,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error().into());
    }
    let user = unsafe { &*(buffer.as_ptr() as *const TOKEN_USER) };
    sid_string(user.User.Sid)
}

pub fn current_user_sid() -> Result<String> {
    let mut token = std::ptr::null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let result = token_sid(token);
    unsafe {
        CloseHandle(token);
    }
    result
}

/// Diagnostics only: exercise the real pipe using a token with Administrators
/// disabled, so the explicit installing-user ACE is actually tested.
pub fn as_unprivileged_caller<T>(operation: impl FnOnce() -> Result<T>) -> Result<T> {
    let mut original = std::ptr::null_mut();
    if unsafe {
        OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_QUERY | TOKEN_DUPLICATE,
            &mut original,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error().into());
    }
    let mut administrators = std::ptr::null_mut();
    if unsafe { ConvertStringSidToSidW(wide("S-1-5-32-544").as_ptr(), &mut administrators) } == 0 {
        unsafe {
            CloseHandle(original);
        }
        return Err(std::io::Error::last_os_error().into());
    }
    let disabled = SID_AND_ATTRIBUTES {
        Sid: administrators,
        Attributes: 0,
    };
    let mut restricted = std::ptr::null_mut();
    let ok = unsafe {
        CreateRestrictedToken(
            original,
            DISABLE_MAX_PRIVILEGE,
            1,
            &disabled,
            0,
            std::ptr::null(),
            0,
            std::ptr::null(),
            &mut restricted,
        )
    };
    let error = std::io::Error::last_os_error();
    unsafe {
        CloseHandle(original);
        LocalFree(administrators);
    }
    if ok == 0 {
        return Err(error.into());
    }
    if unsafe { ImpersonateLoggedOnUser(restricted) } == 0 {
        let error = std::io::Error::last_os_error();
        unsafe {
            CloseHandle(restricted);
        }
        return Err(error.into());
    }
    struct Restore(HANDLE);
    impl Drop for Restore {
        fn drop(&mut self) {
            if unsafe { RevertToSelf() } == 0 {
                std::process::abort();
            }
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
    let restore = Restore(restricted);
    let result = operation();
    drop(restore);
    result
}

pub fn as_anonymous_caller<T>(operation: impl FnOnce() -> Result<T>) -> Result<T> {
    if unsafe { ImpersonateAnonymousToken(GetCurrentThread()) } == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    struct Restore;
    impl Drop for Restore {
        fn drop(&mut self) {
            if unsafe { RevertToSelf() } == 0 {
                std::process::abort();
            }
        }
    }
    let restore = Restore;
    let result = operation();
    drop(restore);
    result
}

/// Read a frame first, then query that pipe message's kernel-captured token.
/// No async suspension or privileged file operation occurs while impersonating.
pub fn pipe_caller_sid(pipe: HANDLE) -> Result<String> {
    if unsafe { ImpersonateNamedPipeClient(pipe) } == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let mut token = std::ptr::null_mut();
    let opened = unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, 1, &mut token) };
    let result = if opened != 0 {
        token_sid(token)
    } else {
        Err(std::io::Error::last_os_error().into())
    };
    if !token.is_null() {
        unsafe {
            CloseHandle(token);
        }
    }
    // Continuing as an impersonated client is unsafe; terminate on OS failure.
    if unsafe { RevertToSelf() } == 0 {
        std::process::abort();
    }
    result
}

fn known_folder(id: &GUID) -> Result<PathBuf> {
    let mut pointer = std::ptr::null_mut();
    let status = unsafe { SHGetKnownFolderPath(id, 0, std::ptr::null_mut(), &mut pointer) };
    if status < 0 {
        bail!("cannot resolve protected Windows directory: {status:#x}");
    }
    let mut length = 0;
    unsafe {
        while *pointer.add(length) != 0 {
            length += 1;
        }
    }
    use std::os::windows::ffi::OsStringExt;
    let path =
        std::ffi::OsString::from_wide(unsafe { std::slice::from_raw_parts(pointer, length) });
    unsafe {
        CoTaskMemFree(pointer as _);
    }
    Ok(path.into())
}

fn trusted_sid(sid: &str) -> bool {
    matches!(sid, "S-1-5-18" | "S-1-5-32-544") || sid == TRUSTED_INSTALLER
}
fn within(path: &Path, root: &Path) -> bool {
    let normalize = |p: &Path| {
        p.to_string_lossy()
            .trim_start_matches(r"\\?\")
            .trim_end_matches('\\')
            .to_ascii_lowercase()
    };
    let path = normalize(path);
    let root = normalize(root);
    path == root || path.starts_with(&(root + "\\"))
}

#[derive(Debug)]
pub struct PathGuards {
    _files: Vec<File>,
}

fn validate_acl(handle: HANDLE, strict: bool, private: bool) -> Result<()> {
    let mut owner = std::ptr::null_mut();
    let mut acl = std::ptr::null_mut();
    let mut descriptor = std::ptr::null_mut();
    let status = unsafe {
        GetSecurityInfo(
            handle,
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner,
            std::ptr::null_mut(),
            &mut acl,
            std::ptr::null_mut(),
            &mut descriptor,
        )
    };
    if status != 0 {
        return Err(std::io::Error::from_raw_os_error(status as i32).into());
    }
    let result = (|| {
        if !trusted_sid(&sid_string(owner)?) {
            bail!("protected path has a non-administrator owner");
        }
        if acl.is_null() {
            bail!("protected path has a NULL DACL");
        }
        for index in 0..unsafe { (*acl).AceCount } as u32 {
            let mut raw = std::ptr::null_mut();
            if unsafe { GetAce(acl, index, &mut raw) } == 0 {
                bail!("cannot read path ACL");
            }
            let header = unsafe { &*(raw as *const ACE_HEADER) };
            if header.AceFlags & INHERIT_ONLY_ACE as u8 != 0 {
                continue;
            }
            // Only unconditional allow / deny ACEs are expected on app files.
            // A callback/object/unknown allow ACE must not silently bypass us.
            if header.AceType == 1 {
                continue;
            }
            if header.AceType != 0 {
                bail!("unsupported ACE in protected path DACL");
            }
            let ace = unsafe { &*(raw as *const ACCESS_ALLOWED_ACE) };
            let trustee = sid_string(&ace.SidStart as *const _ as PSID)?;
            let disallowed = if private {
                ace.Mask != 0
            } else {
                ace.Mask & if strict { WRITE_MASK } else { ANCESTOR_MASK } != 0
            };
            if disallowed && !trusted_sid(&trustee) {
                bail!("protected path grants unsafe access to {trustee}");
            }
        }
        Ok(())
    })();
    unsafe {
        LocalFree(descriptor);
    }
    result
}

/// Inspect every component without following reparse points. Retain no-delete
/// handles to prevent renames while the service relies on the path.
pub fn hold_path(
    path: &Path,
    content_root: &Path,
    private_root: Option<&Path>,
) -> Result<PathGuards> {
    if !path.is_absolute() || !within(path, content_root) {
        bail!("path is outside the protected installation");
    }
    let mut current = PathBuf::new();
    let mut files = Vec::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix)
                if matches!(
                    prefix.kind(),
                    std::path::Prefix::Disk(_) | std::path::Prefix::VerbatimDisk(_)
                ) =>
            {
                current.push(component.as_os_str())
            }
            Component::RootDir => current.push(component.as_os_str()),
            Component::Normal(_) => current.push(component.as_os_str()),
            _ => bail!("protected path must be a local absolute path without traversal"),
        }
        if matches!(component, Component::Prefix(_)) {
            continue;
        }
        let is_file = current == path && path.is_file();
        let handle = unsafe {
            CreateFileW(
                wide(&current).as_ptr(),
                READ_CONTROL | FILE_READ_ATTRIBUTES,
                if is_file {
                    FILE_SHARE_READ
                } else {
                    FILE_SHARE_READ | FILE_SHARE_WRITE
                },
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
                std::ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(std::io::Error::last_os_error())
                .with_context(|| format!("open protected path {}", current.display()));
        }
        let file = unsafe { File::from_raw_handle(handle as _) };
        let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
        if unsafe { GetFileInformationByHandle(handle, &mut info) } == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        if info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            bail!("reparse point in protected path: {}", current.display());
        }
        validate_acl(
            handle,
            within(&current, content_root),
            private_root.is_some_and(|root| within(&current, root)),
        )
        .with_context(|| format!("unsafe permissions on {}", current.display()))?;
        files.push(file);
    }
    Ok(PathGuards { _files: files })
}

pub fn create_private_directory(path: &Path) -> Result<()> {
    let security = SecurityAttributes::from_sddl(PRIVATE_SDDL)?;
    if unsafe { CreateDirectoryW(wide(path).as_ptr(), &security.attrs) } == 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() != Some(ERROR_ALREADY_EXISTS as i32) {
            return Err(error.into());
        }
    }
    Ok(())
}

#[derive(Debug)]
pub struct InstalledPaths {
    pub service: PathBuf,
    pub kernel: PathBuf,
    pub dll_dir: PathBuf,
    pub work_root: PathBuf,
    pub kernel_home: PathBuf,
    _guards: Vec<PathGuards>,
}
impl InstalledPaths {
    pub fn load() -> Result<Self> {
        let service = std::env::current_exe()?;
        let install_root = known_folder(&FOLDERID_ProgramFiles)?.join("Sufe");
        if !within(&service, &install_root) {
            bail!("请先使用面向所有用户的安装包装入 Program Files\\Sufe，开发目录不能注册特权服务");
        }
        let parent = service
            .parent()
            .context("service executable has no parent")?;
        let kernel = parent.join("mihomo.exe");
        let dll = if parent.join("wintun.dll").is_file() {
            parent.join("wintun.dll")
        } else {
            parent.join("binaries/wintun.dll")
        };
        let mut guards = vec![
            hold_path(&service, &install_root, None)?,
            hold_path(&kernel, &install_root, None)?,
            hold_path(&dll, &install_root, None)?,
        ];
        let data_base = known_folder(&FOLDERID_ProgramData)?.join("Sufe");
        create_private_directory(&data_base)?;
        guards.push(hold_path(&data_base, &data_base, None)?);
        let work_root = data_base.join("Service");
        create_private_directory(&work_root)?;
        guards.push(hold_path(&work_root, &data_base, Some(&work_root))?);
        let kernel_home = work_root.join("kernel");
        create_private_directory(&kernel_home)?;
        guards.push(hold_path(&kernel_home, &work_root, Some(&work_root))?);
        Ok(Self {
            service,
            kernel,
            dll_dir: dll.parent().unwrap().to_path_buf(),
            work_root,
            kernel_home,
            _guards: guards,
        })
    }
    pub fn session_directory(&self) -> Result<(PathBuf, PathGuards)> {
        let path = self.work_root.join(uuid::Uuid::new_v4().to_string());
        create_private_directory(&path)?;
        let guard = hold_path(&path, &self.work_root, Some(&self.work_root))?;
        Ok((path, guard))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn canonical_sid_only() {
        assert!(validate_sid("S-1-5-21-1-2-3-1001").is_ok());
        assert!(validate_sid("S-1-5-21-1;cmd.exe").is_err());
        assert!(validate_sid("S-1-5-021-1").is_err());
    }
    #[test]
    fn path_prefix_is_component_bounded() {
        assert!(within(
            Path::new(r"C:\Program Files\Sufe\mihomo.exe"),
            Path::new(r"C:\Program Files\Sufe")
        ));
        assert!(!within(
            Path::new(r"C:\Program Files\Sufe-evil\mihomo.exe"),
            Path::new(r"C:\Program Files\Sufe")
        ));
    }
    #[test]
    fn acl_rejects_user_write_and_private_read() {
        // Pure security descriptor tests exercise actual Win32 ACL decoding.
        let root = known_folder(&FOLDERID_ProgramData)
            .unwrap()
            .join(format!("SufeAclTest-{}", uuid::Uuid::new_v4()));
        let created = create_private_directory(&root);
        if created.is_err() {
            return;
        } // Non-elevated CI cannot create BA-owned directories.
        let guard = hold_path(&root, &root, Some(&root)).unwrap();
        drop(guard);
        let insecure = root.join("writable");
        let security = SecurityAttributes::from_sddl(
            "O:BAG:BAD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;;GW;;;BU)",
        )
        .unwrap();
        assert_ne!(
            unsafe { CreateDirectoryW(wide(&insecure).as_ptr(), &security.attrs) },
            0
        );
        assert!(hold_path(&insecure, &root, None).is_err());
        std::fs::remove_dir(&insecure).unwrap();
        let readable = root.join("readable");
        let security = SecurityAttributes::from_sddl(
            "O:BAG:BAD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;;GR;;;BU)",
        )
        .unwrap();
        assert_ne!(
            unsafe { CreateDirectoryW(wide(&readable).as_ptr(), &security.attrs) },
            0
        );
        assert!(hold_path(&readable, &root, None).is_ok());
        assert!(hold_path(&readable, &root, Some(&root)).is_err());
        std::fs::remove_dir(&readable).unwrap();
        std::fs::remove_dir(&root).unwrap();
    }

    #[test]
    fn rejects_reparse_and_untrusted_owner() {
        let root = known_folder(&FOLDERID_ProgramData)
            .unwrap()
            .join(format!("SufePathTest-{}", uuid::Uuid::new_v4()));
        if create_private_directory(&root).is_err() {
            return;
        }
        let target = root.join("target");
        create_private_directory(&target).unwrap();
        let link = root.join("link");
        std::os::windows::fs::symlink_dir(&target, &link).unwrap();
        assert!(
            hold_path(&link, &root, None).is_err(),
            "reparse points must fail before canonicalization"
        );
        std::fs::remove_dir(&link).unwrap();
        let owned = root.join("user-owned");
        let sid = current_user_sid().unwrap();
        let descriptor = SecurityAttributes::from_sddl(&format!(
            "O:{sid}G:BAD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)"
        ))
        .unwrap();
        assert_ne!(
            unsafe { CreateDirectoryW(wide(&owned).as_ptr(), &descriptor.attrs) },
            0
        );
        assert!(
            hold_path(&owned, &root, None).is_err(),
            "owner can rewrite DACL even without a write ACE"
        );
        std::fs::remove_dir(&owned).unwrap();
        std::fs::remove_dir(&target).unwrap();
        std::fs::remove_dir(&root).unwrap();
    }
}
