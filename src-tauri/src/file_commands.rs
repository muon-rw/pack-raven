use tauri::State;

use crate::commands::AppState;
use crate::files::{self, Entry, FileContent};

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
