//! Explicit administrator diagnostic; never changes IP addresses, DNS, routes
//! or system proxy, and only closes the adapter it creates under a unique name.
use crate::security::{wide, InstalledPaths};
use anyhow::{bail, Result};
use std::{ffi::c_void, path::Path};
use windows_sys::{
    core::GUID,
    Win32::{
        Foundation::{FreeLibrary, HANDLE, HMODULE},
        System::LibraryLoader::{
            GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR,
            LOAD_LIBRARY_SEARCH_SYSTEM32,
        },
    },
};

type CreateAdapter = unsafe extern "system" fn(*const u16, *const u16, *const GUID) -> HANDLE;
type CloseAdapter = unsafe extern "system" fn(HANDLE);
type StartSession = unsafe extern "system" fn(HANDLE, u32) -> HANDLE;
type EndSession = unsafe extern "system" fn(HANDLE);
type DriverVersion = unsafe extern "system" fn() -> u32;

struct Library(HMODULE);
impl Drop for Library {
    fn drop(&mut self) {
        unsafe {
            FreeLibrary(self.0);
        }
    }
}
struct Resource(HANDLE, unsafe extern "system" fn(HANDLE));
impl Drop for Resource {
    fn drop(&mut self) {
        unsafe {
            (self.1)(self.0);
        }
    }
}

fn symbol(library: HMODULE, name: &'static [u8]) -> Result<*const c_void> {
    let function = unsafe { GetProcAddress(library, name.as_ptr()) };
    function
        .map(|function| function as *const c_void)
        .ok_or_else(|| std::io::Error::last_os_error().into())
}

pub fn run(paths: &InstalledPaths) -> Result<()> {
    run_dll(&paths.dll_dir.join("wintun.dll"))
}

fn run_dll(path: &Path) -> Result<()> {
    let raw = unsafe {
        LoadLibraryExW(
            wide(path).as_ptr(),
            std::ptr::null_mut(),
            LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32,
        )
    };
    if raw.is_null() {
        return Err(std::io::Error::last_os_error().into());
    }
    let library = Library(raw);
    let create: CreateAdapter =
        unsafe { std::mem::transmute(symbol(library.0, b"WintunCreateAdapter\0")?) };
    let close: CloseAdapter =
        unsafe { std::mem::transmute(symbol(library.0, b"WintunCloseAdapter\0")?) };
    let start: StartSession =
        unsafe { std::mem::transmute(symbol(library.0, b"WintunStartSession\0")?) };
    let end: EndSession = unsafe { std::mem::transmute(symbol(library.0, b"WintunEndSession\0")?) };
    let version: DriverVersion =
        unsafe { std::mem::transmute(symbol(library.0, b"WintunGetRunningDriverVersion\0")?) };
    let name = format!("SufeProbe-{}", uuid::Uuid::new_v4().simple());
    let adapter = unsafe {
        create(
            wide(&name).as_ptr(),
            wide("Sufe diagnostic").as_ptr(),
            std::ptr::null(),
        )
    };
    if adapter.is_null() {
        bail!(
            "Wintun adapter probe failed: {}",
            std::io::Error::last_os_error()
        );
    }
    let adapter = Resource(adapter, close);
    let session = unsafe { start(adapter.0, 0x20000) };
    if session.is_null() {
        bail!(
            "Wintun ring session probe failed: {}",
            std::io::Error::last_os_error()
        );
    }
    let session = Resource(session, end);
    let driver = unsafe { version() };
    drop(session);
    // Wintun 0.14.1 documents that closing a created adapter removes it.
    drop(adapter);
    println!(
        "{}",
        serde_json::json!({"adapter":name,"adapter_created":true,"ring_session_opened":true,"adapter_closed":true,"driver_version":driver,"routes_modified":false,"dns_modified":false,"proxy_modified":false})
    );
    Ok(())
}
