fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        pin_macos_sidecars();
    }
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

fn pin_macos_sidecars() {
    let target = std::env::var("TARGET").expect("Cargo target");
    assert!(matches!(
        target.as_str(),
        "aarch64-apple-darwin" | "x86_64-apple-darwin"
    ));
    let directory =
        std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("binaries");
    let pins = directory.join(format!("macos-sidecar-pins-{target}.sha256"));
    println!("cargo:rerun-if-changed={}", pins.display());
    let pins = std::fs::read_to_string(pins).expect(
        "run ci/scripts/presign-macos-sidecars.py for this target before building macOS desktop",
    );
    for (name, variable) in [
        ("mihomo", "SUFE_MAC_KERNEL_SHA256"),
        ("xboard-helper", "SUFE_MAC_HELPER_SHA256"),
    ] {
        let expected: Vec<_> = pins
            .lines()
            .filter_map(|line| {
                let fields: Vec<_> = line.split_whitespace().collect();
                (fields.len() == 2 && fields[1] == name).then(|| fields[0])
            })
            .collect();
        assert_eq!(expected.len(), 1, "missing or duplicate sidecar pin");
        let expected = expected[0];
        assert!(expected.len() == 64 && expected.bytes().all(|byte| byte.is_ascii_hexdigit()));
        let binary = directory.join(format!("{name}-{target}"));
        println!("cargo:rerun-if-changed={}", binary.display());
        let output = std::process::Command::new("/usr/bin/shasum")
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("LC_ALL", "C")
            .args(["-a", "256", "--"])
            .arg(binary)
            .output()
            .expect("macOS shasum");
        assert!(output.status.success(), "cannot hash pre-signed sidecar");
        let output = String::from_utf8(output.stdout).expect("SHA256 output");
        assert_eq!(
            output.split_whitespace().next(),
            Some(expected),
            "sidecar changed after pre-signing; regenerate pins"
        );
        println!("cargo:rustc-env={variable}={expected}");
    }
}
