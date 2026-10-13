use std::path::{Path, PathBuf};
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

use crate::process::{encode_powershell, powershell};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DefenderStatus {
    pub active: bool,
    pub exclusions_readable: bool,
    pub missing: Vec<PathBuf>,
}

#[derive(Deserialize)]
struct Probe {
    realtime: Option<bool>,
    exclusions: Option<Vec<Option<String>>>,
}

fn normalize(path: &str) -> String {
    path.trim().trim_end_matches(['\\', '/']).replace('/', "\\").to_lowercase()
}

fn covered(path: &Path, exclusions: &[String]) -> bool {
    let path = normalize(&path.to_string_lossy());
    exclusions.iter().map(|e| normalize(e)).any(|e| path == e || path.starts_with(&format!("{e}\\")))
}

pub fn status(paths: &[PathBuf]) -> DefenderStatus {
    let inactive = DefenderStatus { active: false, exclusions_readable: false, missing: Vec::new() };
    if !cfg!(windows) {
        return inactive;
    }
    let script = "$s = Get-MpComputerStatus -ErrorAction SilentlyContinue; $p = Get-MpPreference -ErrorAction SilentlyContinue; \
                  [pscustomobject]@{ realtime = $s.RealTimeProtectionEnabled; exclusions = @($p.ExclusionPath) } | ConvertTo-Json -Compress";
    let Ok(output) = powershell(script).output() else { return inactive };
    let Ok(probe) = serde_json::from_slice::<Probe>(&output.stdout) else { return inactive };
    if probe.realtime != Some(true) {
        return inactive;
    }
    let exclusions: Vec<String> = probe.exclusions.unwrap_or_default().into_iter().flatten().collect();
    let readable = !exclusions.iter().any(|e| e.starts_with("N/A"));
    let missing = if readable { paths.iter().filter(|p| !covered(p, &exclusions)).cloned().collect() } else { paths.to_vec() };
    DefenderStatus { active: true, exclusions_readable: readable, missing }
}

pub(crate) fn quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "''"))
}

pub fn exclude(paths: &[PathBuf]) -> Result<()> {
    let add = format!("Add-MpPreference -ExclusionPath {}", paths.iter().map(|p| quote(p)).collect::<Vec<_>>().join(","));
    let inner = encode_powershell(&add);
    let elevate = format!(
        "$p = Start-Process powershell -Verb RunAs -Wait -PassThru -WindowStyle Hidden -ArgumentList '-NoProfile','-NonInteractive','-EncodedCommand','{inner}'; exit $p.ExitCode"
    );
    let output = powershell(&elevate).output()?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr);
        if detail.contains("canceled by the user") {
            bail!("You declined the Windows permission prompt, so packraven didn't add any exclusions.");
        }
        bail!("Adding the exclusions failed: {}", detail.trim());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coverage_matches_parent_folders_only() {
        let exclusions = vec!["C:\\Users\\me\\curseforge\\".to_string()];
        assert!(covered(&PathBuf::from("C:/Users/me/curseforge/minecraft/Instances/Pack"), &exclusions));
        assert!(covered(&PathBuf::from("c:\\users\\me\\CurseForge"), &exclusions));
        assert!(!covered(&PathBuf::from("C:\\Users\\me\\curseforge-old"), &exclusions));
    }

    #[test]
    fn quotes_apostrophes() {
        assert_eq!(quote(&PathBuf::from("C:\\Raven's High Fantasy")), "'C:\\Raven''s High Fantasy'");
    }
}
