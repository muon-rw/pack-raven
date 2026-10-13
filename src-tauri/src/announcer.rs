use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};

use crate::commands::{pack_data_dir, AppState};
use crate::{discord, publish};

const CHECK_EVERY: Duration = Duration::from_secs(300);

pub fn start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            check(&app).await;
            tokio::time::sleep(CHECK_EVERY).await;
        }
    });
}

async fn check(app: &AppHandle) {
    let state = app.state::<AppState>();
    let settings = state.settings();
    for pack in &settings.packs {
        let Some(webhook) = settings.webhook(pack) else { continue };
        let releases_path = pack_data_dir(&state.data_dir(), pack).join("releases.json");
        for release in publish::load_releases(&releases_path).into_iter().filter(|r| r.announce && r.announced_at.is_none()) {
            if !discord::is_approved(&release).await {
                continue;
            }
            match discord::announce(pack, &releases_path, &release.version, &webhook).await {
                Ok(()) => {
                    let _ = app.emit("release-announced", &release.version);
                }
                Err(error) => eprintln!("packraven: announcing {} failed: {error:#}", release.version),
            }
        }
    }
}
