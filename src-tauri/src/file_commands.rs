use std::collections::HashSet;
use std::path::PathBuf;

use clipboard_rs::{Clipboard, ClipboardContent, ClipboardContext};
use tauri::{AppHandle, State, Window};
use tauri_plugin_opener::OpenerExt;

use crate::commands::AppState;
use crate::files::{self, Entry, FileContent, Pasted};
use crate::os_drag;

type CmdResult<T> = Result<T, String>;

fn err(error: anyhow::Error) -> String {
    format!("{error:#}")
}

#[tauri::command]
pub fn list_dir(state: State<'_, AppState>, path: String) -> CmdResult<Vec<Entry>> {
    files::list(&state.settings().instance_dir, &path).map_err(err)
}

#[tauri::command]
pub fn read_file(state: State<'_, AppState>, path: String) -> CmdResult<FileContent> {
    files::read(&state.settings().instance_dir, &path).map_err(err)
}

#[tauri::command]
pub fn write_file(state: State<'_, AppState>, path: String, text: String, crlf: bool, bom: bool) -> CmdResult<()> {
    files::write(&state.settings().instance_dir, &path, &text, crlf, bom).map_err(err)
}

#[tauri::command]
pub fn absolute_path(state: State<'_, AppState>, path: String) -> CmdResult<String> {
    Ok(files::resolve(&state.settings().instance_dir, &path).map_err(err)?.to_string_lossy().into_owned())
}

#[tauri::command]
pub fn create_file(state: State<'_, AppState>, path: String) -> CmdResult<()> {
    files::create_file(&state.settings().instance_dir, &path).map_err(err)
}

#[tauri::command]
pub fn create_dir(state: State<'_, AppState>, path: String) -> CmdResult<()> {
    files::create_dir(&state.settings().instance_dir, &path).map_err(err)
}

#[tauri::command]
pub async fn trash_paths(state: State<'_, AppState>, paths: Vec<String>) -> CmdResult<()> {
    let root = state.settings().instance_dir;
    tauri::async_runtime::spawn_blocking(move || files::trash(&root, &paths)).await.map_err(|e| e.to_string())?.map_err(err)
}

#[tauri::command]
pub async fn copy_files(state: State<'_, AppState>, paths: Vec<String>, cut: bool) -> CmdResult<()> {
    let absolute = files::absolute_paths(&state.settings().instance_dir, &paths).map_err(err)?;
    tauri::async_runtime::spawn_blocking(move || {
        let clipboard = ClipboardContext::new().map_err(|e| format!("opening the clipboard: {e}"))?;
        let mut contents = vec![ClipboardContent::Files(absolute)];
        // Explorer reads this Windows shell format to decide whether a paste moves or copies.
        // Finder has no equivalent, so on macOS a cut pastes as a copy outside packraven.
        if cfg!(windows) {
            let effect: u32 = if cut { DROPEFFECT_MOVE } else { 1 };
            contents.push(ClipboardContent::Other("Preferred DropEffect".into(), effect.to_le_bytes().to_vec()));
        }
        clipboard.set(contents).map_err(|e| format!("copying to the clipboard: {e}"))
    })
    .await
    .map_err(|e| e.to_string())?
}

const DROPEFFECT_MOVE: u32 = 2;

// Explorer marks a cut by setting this Windows shell format to DROPEFFECT_MOVE and leaves the
// move to whichever app you paste into.
fn explorer_cut(clipboard: &ClipboardContext) -> bool {
    cfg!(windows)
        && clipboard
            .get_buffer("Preferred DropEffect")
            .ok()
            .and_then(|bytes| Some(u32::from_le_bytes(bytes.get(..4)?.try_into().ok()?)))
            .is_some_and(|effect| effect & DROPEFFECT_MOVE != 0)
}

#[tauri::command]
pub async fn paste_files(state: State<'_, AppState>, dir: String, cut: Vec<String>) -> CmdResult<Pasted> {
    let root = state.settings().instance_dir;
    tauri::async_runtime::spawn_blocking(move || {
        let clipboard = ClipboardContext::new().map_err(|e| format!("opening the clipboard: {e}"))?;
        // clipboard-rs reports a clipboard without files as an error, which isn't one for us.
        let sources: Vec<PathBuf> = clipboard.get_files().unwrap_or_default().into_iter().map(PathBuf::from).collect();
        if sources.is_empty() {
            return Err("The clipboard has no files to paste.".to_string());
        }
        // You may have copied something else since you cut, so we only treat the paste as a move
        // when the clipboard still holds exactly the files you cut.
        let cut_paths: HashSet<PathBuf> = files::absolute_paths(&root, &cut).map_err(err)?.into_iter().map(PathBuf::from).collect();
        let cut_here = !cut.is_empty() && sources.iter().cloned().collect::<HashSet<_>>() == cut_paths;
        let move_requested = cut_here || explorer_cut(&clipboard);
        let pasted = files::paste(&root, &dir, &sources, move_requested).map_err(err)?;
        // Like Explorer, we empty the clipboard after a cut, since its files are no longer there.
        if move_requested {
            let _ = clipboard.clear();
        }
        Ok(pasted)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn rename_path(state: State<'_, AppState>, from: String, to: String) -> CmdResult<()> {
    files::rename(&state.settings().instance_dir, &from, &to).map_err(err)
}

#[tauri::command]
pub fn move_paths(state: State<'_, AppState>, paths: Vec<String>, dir: String) -> CmdResult<Vec<(String, String)>> {
    files::move_into(&state.settings().instance_dir, &paths, &dir).map_err(err)
}

#[tauri::command]
pub async fn import_paths(state: State<'_, AppState>, dir: String, sources: Vec<PathBuf>) -> CmdResult<Pasted> {
    let root = state.settings().instance_dir;
    let requested_move = os_drag::requested_move();
    tauri::async_runtime::spawn_blocking(move || files::import(&root, &dir, &sources, requested_move)).await.map_err(|e| e.to_string())?.map_err(err)
}

// The OS drag loop needs the main thread, and on Windows it blocks that thread until you drop,
// so we start it there and wait for it here.
#[tauri::command]
pub async fn drag_out(app: AppHandle, window: Window, state: State<'_, AppState>, paths: Vec<String>) -> CmdResult<()> {
    let files: Vec<PathBuf> = files::absolute_paths(&state.settings().instance_dir, &paths).map_err(err)?.into_iter().map(PathBuf::from).collect();
    let (sender, receiver) = std::sync::mpsc::channel();
    app.run_on_main_thread(move || {
        let _ = sender.send(os_drag::drag_out(&window, &files).map_err(|e| format!("starting the drag: {e}")));
    })
    .map_err(|e| e.to_string())?;
    tauri::async_runtime::spawn_blocking(move || receiver.recv().unwrap_or(Ok(()))).await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn open_pack_folder(app: AppHandle, state: State<'_, AppState>) -> CmdResult<()> {
    let root = state.settings().instance_dir;
    app.opener().open_path(root.to_string_lossy(), None::<&str>).map_err(|e| e.to_string())
}
