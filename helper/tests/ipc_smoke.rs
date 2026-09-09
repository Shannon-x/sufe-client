//! No installation or socket mutation: the release daemon has no test bypass.
#![cfg(target_os = "macos")]
#[test]
fn shipped_binary_rejects_non_root_even_with_legacy_bypass_variable() {
    if unsafe { libc::geteuid() } == 0 {
        return;
    }
    let result = std::process::Command::new(env!("CARGO_BIN_EXE_xboard-helper"))
        .env("XBOARD_HELPER_ALLOW_NONROOT", "1")
        .output()
        .expect("run helper");
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("must run as root"));
}
