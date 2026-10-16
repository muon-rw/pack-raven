use std::hash::{BuildHasher, RandomState};

fn main() {
    embed_curseforge_key();
    let attributes = tauri_build::Attributes::new().windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest());
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let manifest = std::env::current_dir().unwrap().join("windows-app-manifest.xml");
        println!("cargo:rerun-if-changed=windows-app-manifest.xml");
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
        println!("cargo:rustc-link-arg=/WX");
    }
    tauri_build::try_build(attributes).expect("tauri build");
}

// You set PACKRAVEN_CF_API_KEY when building a release, so the key never goes in the repo. We XOR
// it with a random mask so it isn't plain text in the binary. This only stops casual scraping;
// anyone can still recover the key, so we only embed the read-only API key this way.
fn embed_curseforge_key() {
    println!("cargo:rerun-if-env-changed=PACKRAVEN_CF_API_KEY");
    let key = std::env::var("PACKRAVEN_CF_API_KEY").unwrap_or_default();
    let key = key.trim().as_bytes();
    if key.is_empty() {
        return;
    }
    let mask: Vec<u8> = (0..key.len()).map(|i| RandomState::new().hash_one(i) as u8).collect();
    let masked: Vec<u8> = key.iter().zip(&mask).map(|(k, m)| k ^ m).collect();
    println!("cargo:rustc-env=PACKRAVEN_CF_KEY_MASKED={}", hex(&masked));
    println!("cargo:rustc-env=PACKRAVEN_CF_KEY_MASK={}", hex(&mask));
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
