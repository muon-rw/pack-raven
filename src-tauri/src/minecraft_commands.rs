use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;
use tokio::sync::oneshot;

use crate::commands::AppState;
use crate::jobs::Log;
use crate::launch::Player;
use crate::minecraft_auth::{self, Account, SessionCache};
use crate::pack::Pack;

type CmdResult<T> = Result<T, String>;

#[tauri::command]
pub fn minecraft_account() -> Option<Account> {
    minecraft_auth::account()
}

#[tauri::command]
pub async fn minecraft_sign_in(app: AppHandle, state: State<'_, AppState>) -> CmdResult<Option<Account>> {
    let (cancel_tx, cancel_rx) = oneshot::channel();
    if let Some(earlier) = state.minecraft_sign_in.lock().unwrap().replace(cancel_tx) {
        let _ = earlier.send(());
    }
    let open = |url: &str| app.opener().open_url(url, None::<&str>).map_err(|e| anyhow::anyhow!("couldn't open your browser: {e}"));
    let Some(session) = minecraft_auth::sign_in(open, cancel_rx).await.map_err(|e| format!("{e:#}"))? else { return Ok(None) };
    let account = session.account.clone();
    *state.minecraft.lock().await = Some(session);
    Ok(Some(account))
}

#[tauri::command]
pub fn minecraft_sign_in_cancel(state: State<'_, AppState>) {
    if let Some(cancel) = state.minecraft_sign_in.lock().unwrap().take() {
        let _ = cancel.send(());
    }
}

#[tauri::command]
pub async fn minecraft_sign_out(state: State<'_, AppState>) -> CmdResult<()> {
    minecraft_auth::sign_out(&state.minecraft).await.map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub fn set_minecraft_online_mode(state: State<'_, AppState>, online: bool) -> CmdResult<()> {
    state.update_settings(|s| s.minecraft_online_mode = online).map_err(|e| format!("{e:#}"))
}

pub struct TestPlayer {
    pub player: Player,
    pub renewal_failure: Option<String>,
}

// A lapsed sign-in shouldn't block a test run, so we fall back to offline and tell you why.
pub async fn test_player(cache: &SessionCache, pack: &Pack, log: &Log) -> TestPlayer {
    let offline = Player::offline(&pack.config.test.username);
    if minecraft_auth::account().is_none() {
        return TestPlayer { player: offline, renewal_failure: None };
    }
    match minecraft_auth::session(cache).await {
        Ok(session) => TestPlayer { player: session.player(), renewal_failure: None },
        Err(e) => {
            let reason = format!("{e:#}");
            log.info(format!("Couldn't renew your Minecraft sign-in, so this run is offline as {}: {reason}", offline.name));
            TestPlayer { player: offline, renewal_failure: Some(reason) }
        }
    }
}
