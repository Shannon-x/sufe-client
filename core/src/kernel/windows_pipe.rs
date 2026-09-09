//! The named-pipe peer must be the process SCM registered for our service.
//! Request only data-write access: GENERIC_WRITE also grants the unrelated
//! FILE_CREATE_PIPE_INSTANCE right and would weaken the server's DACL.
#![cfg(windows)]

use std::{ffi::OsStr, io, os::windows::ffi::OsStrExt, os::windows::io::RawHandle};
use tokio::net::windows::named_pipe::NamedPipeClient;
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE},
    Storage::FileSystem::{
        CreateFileW, FILE_FLAG_OVERLAPPED, FILE_GENERIC_READ, FILE_WRITE_DATA, OPEN_EXISTING,
        SECURITY_IDENTIFICATION, SECURITY_SQOS_PRESENT,
    },
    System::{Pipes::GetNamedPipeServerProcessId, Services::*},
};

pub const SERVICE_NAME: &str = "xboard-svc";

/// Capture the interactive user's SID before UAC can switch identities.
pub fn current_user_sid() -> io::Result<String> {
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::{
            Authorization::ConvertSidToStringSidW, GetTokenInformation, TokenUser, TOKEN_QUERY,
            TOKEN_USER,
        },
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };
    let mut token = std::ptr::null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut required = 0;
    unsafe {
        GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut required);
    }
    if required == 0 {
        unsafe {
            CloseHandle(token);
        }
        return Err(io::Error::last_os_error());
    }
    let mut buffer = vec![0usize; (required as usize).div_ceil(std::mem::size_of::<usize>())];
    let ok = unsafe {
        GetTokenInformation(
            token,
            TokenUser,
            buffer.as_mut_ptr() as _,
            required,
            &mut required,
        )
    };
    let error = io::Error::last_os_error();
    unsafe {
        CloseHandle(token);
    }
    if ok == 0 {
        return Err(error);
    }
    let user = unsafe { &*(buffer.as_ptr() as *const TOKEN_USER) };
    let mut pointer = std::ptr::null_mut();
    if unsafe { ConvertSidToStringSidW(user.User.Sid, &mut pointer) } == 0 {
        return Err(io::Error::last_os_error());
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

fn wide(value: &str) -> Vec<u16> {
    OsStr::new(value).encode_wide().chain(Some(0)).collect()
}

/// Open the installed service, refusing a pipe squatter before sending secrets.
pub fn open_service_pipe(path: &str) -> io::Result<NamedPipeClient> {
    if path != super::ipc::SVC_PIPE_PATH {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "unexpected service pipe",
        ));
    }
    let name = wide(path);
    let handle = unsafe {
        CreateFileW(
            name.as_ptr(),
            FILE_GENERIC_READ | FILE_WRITE_DATA,
            0,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    if let Err(error) = verify_server(handle) {
        unsafe {
            CloseHandle(handle);
        }
        return Err(error);
    }
    // from_raw_handle takes ownership even if registering the handle fails.
    unsafe { NamedPipeClient::from_raw_handle(handle as RawHandle) }
}

fn verify_server(pipe: HANDLE) -> io::Result<()> {
    let mut pid = 0;
    if unsafe { GetNamedPipeServerProcessId(pipe, &mut pid) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let expected = service_pid()?;
    if pid == 0 || pid != expected {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "named pipe server is not the registered Sufe service",
        ));
    }
    Ok(())
}

fn service_pid() -> io::Result<u32> {
    let manager = unsafe { OpenSCManagerW(std::ptr::null(), std::ptr::null(), SC_MANAGER_CONNECT) };
    if manager.is_null() {
        return Err(io::Error::last_os_error());
    }
    let name = wide(SERVICE_NAME);
    let service = unsafe { OpenServiceW(manager, name.as_ptr(), SERVICE_QUERY_STATUS) };
    unsafe {
        CloseServiceHandle(manager);
    }
    if service.is_null() {
        return Err(io::Error::last_os_error());
    }
    let mut status: SERVICE_STATUS_PROCESS = unsafe { std::mem::zeroed() };
    let mut required = 0;
    let ok = unsafe {
        QueryServiceStatusEx(
            service,
            SC_STATUS_PROCESS_INFO,
            &mut status as *mut _ as *mut u8,
            std::mem::size_of_val(&status) as u32,
            &mut required,
        )
    };
    unsafe {
        CloseServiceHandle(service);
    }
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    if status.dwCurrentState != SERVICE_RUNNING || status.dwProcessId == 0 {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "Sufe service is not running",
        ));
    }
    Ok(status.dwProcessId)
}

#[cfg(test)]
mod tests {
    #[test]
    fn refuses_other_pipe_before_opening() {
        let result = super::open_service_pipe(r"\\.\pipe\not-sufe");
        assert_eq!(
            result.unwrap_err().kind(),
            std::io::ErrorKind::PermissionDenied
        );
    }
    #[test]
    fn minimal_access_excludes_pipe_instance_creation() {
        use windows_sys::Win32::Storage::FileSystem::*;
        assert_eq!((FILE_GENERIC_READ | FILE_WRITE_DATA) & FILE_APPEND_DATA, 0);
    }
}
