use anyhow::{bail, Context, Result};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::git;
use crate::http::CLIENT;

const RELEASE_API: &str = "https://api.github.com/repos/git-for-windows/git/releases/latest";

#[derive(Deserialize)]
struct Release {
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    digest: Option<String>,
}

/// Installs Git and returns what the user should do next.
pub async fn install() -> Result<String> {
    if git::version().is_some() {
        return Ok("Git is already installed.".into());
    }
    if cfg!(target_os = "macos") {
        std::process::Command::new("xcode-select").arg("--install").spawn().context("starting the macOS developer tools installer")?;
        return Ok("Finish the install in the macOS dialog, then choose Check again.".into());
    }
    let installer = download_installer().await?;
    let result = tauri::async_runtime::spawn_blocking({
        let installer = installer.clone();
        move || run_elevated(&installer)
    })
    .await?;
    let _ = std::fs::remove_file(&installer);
    result?;
    match git::version() {
        Some(version) => Ok(format!("Installed Git {version}.")),
        None => bail!("The Git installer finished, but packraven still can't find git. Restart packraven and try again."),
    }
}

pub(crate) async fn download_installer() -> Result<std::path::PathBuf> {
    let suffix = if cfg!(target_arch = "aarch64") { "-arm64.exe" } else { "-64-bit.exe" };
    let release: Release = CLIENT.get(RELEASE_API).send().await?.error_for_status()?.json().await.context("reading the Git for Windows release list")?;
    let asset = release.assets.into_iter().find(|a| a.name.starts_with("Git-") && a.name.ends_with(suffix)).context("the latest Git for Windows release has no installer for this PC")?;
    let expected = asset.digest.as_deref().and_then(|d| d.strip_prefix("sha256:")).context("GitHub didn't list a checksum for the Git installer")?.to_lowercase();
    let bytes = CLIENT.get(&asset.browser_download_url).send().await?.error_for_status()?.bytes().await.context("downloading the Git installer")?;
    if hex::encode(Sha256::digest(&bytes)) != expected {
        bail!("The downloaded Git installer doesn't match GitHub's checksum, so packraven didn't run it.");
    }
    let path = std::env::temp_dir().join(&asset.name);
    std::fs::write(&path, &bytes)?;
    Ok(path)
}

fn run_elevated(installer: &std::path::Path) -> Result<()> {
    let script = format!(
        "$p = Start-Process -FilePath {} -ArgumentList '/VERYSILENT','/NORESTART','/NOCANCEL','/SP-','/SUPPRESSMSGBOXES' -Verb RunAs -Wait -PassThru; exit $p.ExitCode",
        crate::defender::quote(installer)
    );
    let output = crate::process::powershell(&script).output()?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr);
        if detail.contains("canceled by the user") {
            bail!("You declined the Windows permission prompt, so packraven didn't install Git.");
        }
        let detail = detail.trim();
        if detail.is_empty() {
            bail!("The Git installer exited with {}.", output.status);
        }
        bail!("The Git installer failed: {detail}");
    }
    Ok(())
}
