//! CI renderer of the same installer used by the macOS application.
//! May be built with `rustc --edition 2021` without building the workspace.
#[path = "../../desktop/src-tauri/src/helper_install_script.rs"]
mod installer;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let script = match args.as_slice() {
        [mode] if mode == "uninstall" => installer::uninstall_script(),
        [mode, helper, kernel, helper_hash, kernel_hash, owner] if mode == "install" => {
            installer::install_script(
                std::path::Path::new(helper), std::path::Path::new(kernel),
                helper_hash, kernel_hash, owner.parse()?,
            )?
        }
        _ => return Err("usage: render-install install HELPER KERNEL HELPER_SHA KERNEL_SHA OWNER_UID | uninstall".into()),
    };
    print!("{script}");
    Ok(())
}
