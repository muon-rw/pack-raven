use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;
use tokio::sync::oneshot;

use crate::commands::AppState;
use crate::launch::Player;
use crate::minecraft_auth::{self, Account, SessionCache};

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

// Mojang only allows launching the game for an account whose ownership we've verified, so there's no offline fallback.
pub async fn signed_in_player(cache: &SessionCache) -> anyhow::Result<Player> {
    if minecraft_auth::account().is_none() {
        anyhow::bail!("{SIGN_IN_FIRST}");
    }
    Ok(minecraft_auth::session(cache).await?.player())
}

pub const SIGN_IN_FIRST: &str = "Sign in to Minecraft to launch the client.";
