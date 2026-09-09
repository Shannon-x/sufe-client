fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        let target = std::env::var("TARGET").expect("Cargo target");
        let binary = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap())
            .join("binaries")
            .join(format!("mihomo-{target}"));
        println!("cargo:rerun-if-changed={}", binary.display());
        let output = std::process::Command::new("sha256sum")
            .arg(&binary)
            .output()
            .expect("Linux builds require coreutils sha256sum");
        assert!(
            output.status.success(),
            "download the pinned Linux sidecar before building"
        );
        let output = String::from_utf8(output.stdout).expect("sha256sum output");
        let digest = output.split_whitespace().next().expect("SHA256 digest");
        assert!(digest.len() == 64 && digest.bytes().all(|b| b.is_ascii_hexdigit()));
        println!("cargo:rustc-env=SUFE_LINUX_KERNEL_SHA256={digest}");
    }
    tauri_build::build()
}
