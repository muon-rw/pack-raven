use serde_json::json;
use tauri::State;

use crate::changelog;
use crate::commands::AppState;
use crate::discord;
use crate::sides::MetadataCache;
use crate::pack::Pack;
use crate::publish;

type CmdResult<T> = Result<T, String>;

fn err(error: anyhow::Error) -> String {
    format!("{error:#}")
}

#[tauri::command]
pub fn get_webhook(state: State<'_, AppState>) -> Option<String> {
    let settings = state.settings();
    settings.webhook(&settings.instance_dir)
}

#[tauri::command]
pub fn set_webhook(state: State<'_, AppState>, url: String) -> CmdResult<()> {
    if !url.trim().is_empty() {
        discord::validate_webhook(url.trim()).map_err(err)?;
    }
    state
        .update_settings(|s| {
            let pack = s.instance_dir.clone();
            s.set_webhook(&pack, &url);
        })
        .map_err(err)
}

#[tauri::command]
pub async fn discord_test(state: State<'_, AppState>) -> CmdResult<()> {
    let settings = state.settings();
    let webhook = settings.webhook(&settings.instance_dir).ok_or("Add a webhook URL first.")?;
    let pack = Pack::open(&settings.instance_dir).map_err(err)?;
    let message = json!({
        "username": pack.name(),
        "content": format!("packraven is connected. We'll post release announcements for {} here.", pack.name()),
        "allowed_mentions": { "parse": [] },
    });
    discord::send(&webhook, &message).await.map(|_| ()).map_err(err)
}

#[tauri::command]
pub async fn announce_release(state: State<'_, AppState>, version: String) -> CmdResult<()> {
    let settings = state.settings();
    let webhook = settings.webhook(&settings.instance_dir).ok_or("Add a Discord webhook in Pack settings first.")?;
    discord::announce(&settings.instance_dir, &state.pack_data().join("releases.json"), &version, &webhook).await.map_err(err)
}

#[tauri::command]
pub async fn discohook_link(state: State<'_, AppState>, version: String) -> CmdResult<String> {
    let pack = Pack::open(&state.settings().instance_dir).map_err(err)?;
    let release = publish::load_releases(&state.pack_data().join("releases.json"))
        .into_iter()
        .find(|r| r.version == version)
        .ok_or_else(|| format!("You haven't published release {version} from this machine."))?;
    let message = discord::message(&discord::announcement(&pack, &release).await, &pack.config.discord);
    discord::discohook_link(&message).await.map_err(err)
}

#[tauri::command]
pub async fn discohook_preview(state: State<'_, AppState>, version: String) -> CmdResult<String> {
    let pack = Pack::open(&state.settings().instance_dir).map_err(err)?;
    let mut cache = MetadataCache::load(&state.cache_path());
    let diff = changelog::mod_diff(&pack, &pack.lock, &mut cache, &state.pack_data().join(changelog::BASELINE_FILE)).await.map_err(err)?;
    let project = discord::project(pack.config.curseforge.project_id, &pack.config.author).await;
    let announcement = discord::Announcement {
        name: pack.name(),
        version,
        changelog: changelog::render(&changelog::read_notes(&pack), &diff),
        file_url: project.url.clone(),
        project,
        game: discord::game(&pack),
    };
    discord::discohook_link(&discord::message(&announcement, &pack.config.discord)).await.map_err(err)
}
