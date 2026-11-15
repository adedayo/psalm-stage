use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=icons/icon.png");
    println!("cargo:rerun-if-changed=icons/icon.icns");
    println!("cargo:rerun-if-changed=icons/icon.ico");
    println!("cargo:rerun-if-changed=icons/32x32.png");
    println!("cargo:rerun-if-changed=icons/128x128.png");
    println!("cargo:rerun-if-changed=icons/128x128@2x.png");

    // Ensure Tauri's externalBin check passes during headless `cargo test --workspace`
    // even before `scripts/prepare-cli-sidecar.mjs` runs for packaging.
    if let (Ok(target), Ok(profile)) = (std::env::var("TARGET"), std::env::var("PROFILE")) {
        let ext = if target.contains("windows") { ".exe" } else { "" };
        let sidecar = PathBuf::from("binaries").join(format!("psalm-stage-cli-{target}{ext}"));
        if profile == "debug" && !sidecar.exists() {
            let _ = std::fs::create_dir_all("binaries");
            let _ = std::fs::write(&sidecar, b"");
        }
    }

    tauri_build::build()
}
