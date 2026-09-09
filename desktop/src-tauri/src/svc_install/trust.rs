//! Validate and lock the elevation target before handing it to UAC.
//! This applies the same owner, write-ACL and reparse policy as the broker;
//! checking only inside an elevated executable would be too late.
use anyhow::{bail, Context, Result};
use std::{
    fs::File,
    os::windows::{
        ffi::{OsStrExt, OsStringExt},
        io::FromRawHandle,
    },
    path::{Component, Path, PathBuf},
};
use windows_sys::Win32::{
    Foundation::{LocalFree, HANDLE, INVALID_HANDLE_VALUE},
    Security::{Authorization::*, *},
    Storage::FileSystem::*,
    System::Com::CoTaskMemFree,
    UI::Shell::{FOLDERID_ProgramFiles, SHGetKnownFolderPath},
};

pub struct ElevationGuard {
    _handles: Vec<File>,
}

fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

fn normalized(path: &Path) -> String {
    path.to_string_lossy()
        .trim_start_matches(r"\\?\")
        .replace('/', "\\")
        .to_ascii_lowercase()
}

fn program_files() -> Result<PathBuf> {
    let mut raw = std::ptr::null_mut();
    if unsafe { SHGetKnownFolderPath(&FOLDERID_ProgramFiles, 0, std::ptr::null_mut(), &mut raw) }
        < 0
        || raw.is_null()
    {
        bail!("无法读取受保护的 Program Files 路径");
    }
    let mut length = 0;
    unsafe {
        while *raw.add(length) != 0 {
            length += 1;
        }
    }
    let path = PathBuf::from(std::ffi::OsString::from_wide(unsafe {
        std::slice::from_raw_parts(raw, length)
    }));
    unsafe { CoTaskMemFree(raw as _) };
    Ok(path)
}

fn sid_text(sid: PSID) -> Result<String> {
    let mut raw = std::ptr::null_mut();
    if sid.is_null()
        || unsafe { IsValidSid(sid) } == 0
        || unsafe { ConvertSidToStringSidW(sid, &mut raw) } == 0
    {
        bail!("安装目录包含无效的安全标识符");
    }
    let mut length = 0;
    unsafe {
        while *raw.add(length) != 0 {
            length += 1;
        }
    }
    let value = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(raw, length) });
    unsafe { LocalFree(raw as _) };
    Ok(value)
}

fn trusted(sid: &str) -> bool {
    matches!(
        sid,
        "S-1-5-18"
            | "S-1-5-32-544"
            | "S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464"
    )
}

fn validate_acl(handle: HANDLE, application_path: bool) -> Result<()> {
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
        if !trusted(&sid_text(owner)?) || acl.is_null() {
            bail!("安装目录所有者或权限不可信");
        }
        // Ancestors may permit creating sibling folders, but never replacing
        // or changing ownership/permissions of our protected directory.
        let mut forbidden = 0x10000 | 0x40000 | 0x80000 | 0x40000000 | 0x10000000 | 0x40;
        if application_path {
            forbidden |= 0x2 | 0x4 | 0x10 | 0x100;
        }
        for index in 0..unsafe { (*acl).AceCount } as u32 {
            let mut raw = std::ptr::null_mut();
            if unsafe { GetAce(acl, index, &mut raw) } == 0 {
                bail!("无法读取安装目录权限");
            }
            let header = unsafe { &*(raw as *const ACE_HEADER) };
            if header.AceFlags & INHERIT_ONLY_ACE as u8 != 0 || header.AceType == 1 {
                continue;
            }
            if header.AceType != 0 {
                bail!("安装目录使用了不支持的访问控制规则");
            }
            let entry = unsafe { &*(raw as *const ACCESS_ALLOWED_ACE) };
            if entry.Mask & forbidden != 0
                && !trusted(&sid_text(&entry.SidStart as *const _ as PSID)?)
            {
                bail!("普通用户可以修改服务安装文件，已拒绝提权");
            }
        }
        Ok(())
    })();
    unsafe { LocalFree(descriptor) };
    result
}

pub fn validate(executable: &Path) -> Result<ElevationGuard> {
    let root = program_files()?.join("Sufe");
    if normalized(executable) != normalized(&root.join("xboard-svc.exe")) {
        bail!("TUN 服务需要 Program Files\\Sufe 中的正式安装版本，开发目录不能提权");
    }
    let mut current = PathBuf::new();
    let mut handles = Vec::new();
    for component in executable.components() {
        match component {
            Component::Prefix(prefix)
                if matches!(
                    prefix.kind(),
                    std::path::Prefix::Disk(_) | std::path::Prefix::VerbatimDisk(_)
                ) =>
            {
                current.push(component.as_os_str());
                continue;
            }
            Component::RootDir | Component::Normal(_) => current.push(component.as_os_str()),
            _ => bail!("服务路径不能包含重定向或相对路径"),
        }
        let handle = unsafe {
            CreateFileW(
                wide(&current).as_ptr(),
                READ_CONTROL | FILE_READ_ATTRIBUTES,
                if current == executable {
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
            return Err(std::io::Error::last_os_error()).context("无法锁定提权服务文件");
        }
        let file = unsafe { File::from_raw_handle(handle as _) };
        let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
        if unsafe { GetFileInformationByHandle(handle, &mut info) } == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        if info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            bail!("服务路径包含 reparse point，已拒绝提权");
        }
        let application_path = normalized(&current) == normalized(&root) || current == executable;
        validate_acl(handle, application_path)?;
        handles.push(file);
    }
    Ok(ElevationGuard { _handles: handles })
}

#[cfg(test)]
mod tests {
    #[test]
    fn user_writable_location_is_rejected_before_elevation() {
        assert!(super::validate(&std::env::temp_dir().join("xboard-svc.exe")).is_err());
    }

    #[test]
    fn installed_service_passes_the_same_pre_elevation_boundary() {
        let installed = super::program_files().unwrap().join("Sufe/xboard-svc.exe");
        if installed.is_file() {
            assert!(super::validate(&installed).is_ok());
        }
    }
}
