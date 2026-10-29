use std::path::Path;

use anyhow::{bail, Context, Result};

use crate::git;
use crate::instance::Instance;
use crate::jobs::Log;
use crate::modsync;
use crate::pack::{Loader, Lockfile, LOCKFILE};
use crate::remote;
use crate::setup;

pub async fn join(dir: &Path, remote_url: &str, log: &Log) -> Result<String> {
    let remote_url = remote_url.trim();
    if remote_url.is_empty() {
        bail!("Enter the pack's git remote first.");
    }
    if dir.join(".git").exists() {
        bail!("{} is already a git repository. Create a new profile in CurseForge to join the pack into.", dir.display());
    }
    let profile = Instance::read(dir)?;
    git::run(dir, &["init", "-q"])?;
    log.info("Created a git repository");
    // Until the checkout, we've only added .git, so we remove it to leave the profile as you created it.
    let upstream = match fetch_pack(dir, remote_url, &profile, log).await {
        Ok(upstream) => upstream,
        Err(error) => {
            let _ = std::fs::remove_dir_all(dir.join(".git"));
            return Err(error);
        }
    };
    let branch = check_out(dir, &upstream, log)?;

    git::run(dir, &["config", "core.hooksPath", ".githooks"])?;
    setup::link_hooks(dir)?;
    log.info("Linked the sync hooks in .githooks");
    let lock = Lockfile::parse(&std::fs::read_to_string(dir.join(LOCKFILE))?)?;
    modsync::restore(dir, log).await.context("To retry the downloads, choose this profile under Track a CurseForge instance in Add pack")?;
    Ok(format!("Joined. packraven checked out {branch} and placed the {} files in its mod list.", lock.files.len()))
}

async fn fetch_pack(dir: &Path, remote_url: &str, profile: &Instance, log: &Log) -> Result<String> {
    git::run(dir, &["remote", "add", "origin", remote_url])?;
    remote::run_logged(dir, &["fetch", "origin"], log).await?;
    if git::run(dir, &["branch", "-r"])?.trim().is_empty() {
        bail!("The remote has no commits yet. Set the pack up with Add pack on the machine that has it, push, then join again.");
    }
    remote::run_logged(dir, &["remote", "set-head", "origin", "--auto"], log).await?;
    let upstream = git::run(dir, &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"])?.trim().to_string();
    let Some(text) = git::show(dir, &upstream, LOCKFILE) else {
        bail!("{upstream} has no {LOCKFILE}, so it isn't a packraven pack. Set it up with Add pack on the machine that has it, push, then join again.");
    };
    check_versions(&Lockfile::parse(&text)?, profile)?;
    Ok(upstream)
}

// The automatic mod list update copies the profile's versions into mods.lock.json, so joining with
// different ones would change the pack for everyone on your next commit.
fn check_versions(pack: &Lockfile, profile: &Instance) -> Result<()> {
    let fits = |wanted: &Option<String>, have: &Option<String>| wanted.is_none() || wanted == have;
    let loader_fits = pack.loader.is_none() || pack.loader == profile.loader;
    if fits(&pack.minecraft, &profile.minecraft) && loader_fits {
        return Ok(());
    }
    let describe = |minecraft: &Option<String>, loader: &Option<Loader>| {
        format!("Minecraft {} with {}", minecraft.as_deref().unwrap_or("(any version)"), loader.as_ref().map_or_else(|| "no mod loader".into(), ToString::to_string))
    };
    bail!(
        "The pack needs {}, but this profile has {}. Create a CurseForge profile with the pack's versions, then join again.",
        describe(&pack.minecraft, &pack.loader),
        describe(&profile.minecraft, &profile.loader)
    )
}

fn check_out(dir: &Path, upstream: &str, log: &Log) -> Result<String> {
    let tracked = git::run(dir, &["ls-tree", "-r", "-z", "--name-only", upstream])?;
    for path in tracked.split('\0').filter(|p| !p.is_empty() && dir.join(p).is_file()) {
        log.info(format!("Replaced {path} with the pack's version"));
    }
    let branch = upstream.strip_prefix("origin/").unwrap_or(upstream).to_string();
    // CurseForge writes files like options.txt into a new profile, and git won't overwrite them without -f.
    git::run(dir, &["checkout", "-q", "-f", "-B", &branch, "--track", upstream])?;
    log.info(format!("Checked out {branch}"));
    Ok(branch)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use sha1::{Digest, Sha1};

    use super::*;

    fn git(dir: &Path, args: &[&str]) {
        let mut all = vec!["-c", "user.name=Test", "-c", "user.email=test@example.com"];
        all.extend(args);
        git::run(dir, &all).unwrap();
    }

    fn write_instance(dir: &Path, loader: &str, mods: &[&str]) {
        let addons: Vec<_> = mods
            .iter()
            .enumerate()
            .map(|(i, name)| {
                let sha1 = hex::encode(Sha1::digest(name.as_bytes()));
                serde_json::json!({ "addonID": i + 1, "installedFile": { "id": i + 10, "fileName": format!("{name}.jar"), "downloadUrl": "http://127.0.0.1:9/unreachable", "hashes": [{ "type": 1, "value": sha1 }] } })
            })
            .collect();
        let instance = serde_json::json!({ "name": "Profile", "gameVersion": "1.21.1", "baseModLoader": { "name": loader }, "installedAddons": addons });
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("minecraftinstance.json"), instance.to_string()).unwrap();
    }

    fn put_mod(dir: &Path, name: &str) {
        std::fs::create_dir_all(dir.join("mods")).unwrap();
        std::fs::write(dir.join("mods").join(format!("{name}.jar")), name).unwrap();
    }

    fn mod_names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir.join("mods")).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        names
    }

    // A pushed pack with one mod, and an empty profile on another machine with options.txt in the way.
    fn shared_pack(name: &str, profile_loader: &str) -> (PathBuf, PathBuf, tokio::runtime::Runtime) {
        let root = std::env::temp_dir().join(format!("packraven-join-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        git(&root, &["init", "-q", "--bare", "origin.git"]);
        git(&root, &["clone", "-q", "origin.git", "theirs"]);
        let theirs = root.join("theirs");
        std::fs::write(theirs.join(".gitignore"), "mods/*.jar\nminecraftinstance.json\n").unwrap();
        std::fs::write(theirs.join("options.txt"), "pack").unwrap();
        write_instance(&theirs, "neoforge-21.1.1", &["base"]);
        put_mod(&theirs, "base");
        runtime.block_on(modsync::update_mod_list(&theirs)).unwrap();
        git(&theirs, &["add", "-A"]);
        git(&theirs, &["commit", "-q", "-m", "Initial"]);
        git(&theirs, &["push", "-q", "-u", "origin", "HEAD"]);

        let profile = root.join("profile");
        write_instance(&profile, profile_loader, &[]);
        std::fs::write(profile.join("options.txt"), "profile").unwrap();
        (root, profile, runtime)
    }

    fn remote(root: &Path) -> String {
        root.join("origin.git").to_string_lossy().into_owned()
    }

    #[test]
    fn join_checks_out_the_pack_over_the_profile() {
        let (root, profile, runtime) = shared_pack("joins", "neoforge-21.1.1");
        // We can't download in a test, so the profile already has the pack's mod.
        put_mod(&profile, "base");
        put_mod(&profile, "stray");

        runtime.block_on(join(&profile, &remote(&root), &Log::console("join"))).unwrap();
        assert_eq!(std::fs::read_to_string(profile.join("options.txt")).unwrap(), "pack");
        assert_eq!(mod_names(&profile), ["base.jar"]);
        assert!(git::changed_files(&profile).is_empty(), "{:?}", git::changed_files(&profile));
        assert_eq!(git::run(&profile, &["config", "core.hooksPath"]).unwrap().trim(), ".githooks");
        assert_eq!(git::ahead_behind(&profile), Some((0, 0)));
    }

    #[test]
    fn join_leaves_a_profile_with_other_versions_untouched() {
        let (root, profile, runtime) = shared_pack("versions", "neoforge-21.1.2");
        let error = runtime.block_on(join(&profile, &remote(&root), &Log::console("join"))).unwrap_err();
        assert!(format!("{error:#}").contains("The pack needs Minecraft 1.21.1 with neoforge-21.1.1"), "{error:#}");
        assert!(!profile.join(".git").exists());
        assert_eq!(std::fs::read_to_string(profile.join("options.txt")).unwrap(), "profile");
    }

    #[test]
    fn failed_downloads_leave_the_mods_for_setup_to_retry() {
        let (root, profile, runtime) = shared_pack("retry", "neoforge-21.1.1");
        assert!(runtime.block_on(join(&profile, &remote(&root), &Log::console("join"))).is_err());
        assert_eq!(modsync::missing_count(&profile), 1);
        assert!(git::changed_files(&profile).is_empty());
    }
}
