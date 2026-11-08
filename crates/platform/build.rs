fn main() {
    println!("cargo:rerun-if-env-changed=PARABOLICA_ASIO_SDK_DIR");
    #[cfg(windows)]
    if std::env::var_os("CARGO_FEATURE_ASIO").is_some() {
        use sha2::{Digest, Sha256};
        let sdk = std::path::PathBuf::from(std::env::var_os("PARABOLICA_ASIO_SDK_DIR")
            .expect("ASIO needs pinned SDK 2.3.4. Run scripts/setup-asio.ps1; see docs/implementation/asio.md"));
        for (file, expected) in [
            (
                "asio.h",
                "93d2f80d49b4ac9c963514e7c1307ab5111d8104246192233745beb058534ea1",
            ),
            (
                "asiosys.h",
                "f44a1ed0fd7c056ff047aeebc4c05a8c185f0585a24d8debc55c70107777dbef",
            ),
            (
                "iasiodrv.h",
                "86ad4788452c74823653cf57d4bd557c48bb6542d7e6eb7fff80102c705a066d",
            ),
        ] {
            let path = sdk.join("common").join(file);
            println!("cargo:rerun-if-changed={}", path.display());
            let bytes = std::fs::read(&path).expect("ASIO SDK header missing");
            assert_eq!(
                format!("{:x}", Sha256::digest(&bytes)),
                expected,
                "ASIO SDK 2.3.4 header mismatch: {}",
                path.display()
            );
        }
        let bridge = "src/windows/asio/bridge.cpp";
        println!("cargo:rerun-if-changed={bridge}");
        cc::Build::new()
            .cpp(true)
            .std("c++17")
            .include(sdk.join("common"))
            .file(bridge)
            .warnings_into_errors(true)
            .compile("parabolica_asio");
        println!("cargo:rustc-link-lib=ole32");
        println!("cargo:rustc-link-lib=user32");
    }
}
