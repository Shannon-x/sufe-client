use crate::error::CommandResult;

#[tauri::command]
pub fn app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[tauri::command]
pub fn core_version() -> CommandResult<String> {
    Ok(xboard_core::version())
}
