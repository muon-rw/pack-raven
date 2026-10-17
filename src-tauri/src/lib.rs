mod announcer;
mod archive;
mod associations;
mod changelog;
mod commands;
mod credentials;
mod curseforge;
mod decompile;
mod defender;
mod discord;
mod discord_commands;
mod export;
mod file_commands;
mod files;
mod git;
mod git_commands;
mod git_install;
mod history;
mod gitops;
mod http;
mod instance_locks;
mod jobs;
mod join;
mod launch;
mod logs;
mod mirror;
mod mod_commands;
mod mod_list;
mod mod_updates;
mod modrinth;
mod modsync;
mod nbt;
mod os_drag;
mod pack;
mod pack_commands;
mod process;
mod publish;
mod remote;
mod settings;
mod setup;
mod sides;
mod system_commands;
mod testrun;
mod watcher;
#[cfg(test)]
mod live_tests;

use std::sync::{Arc, Mutex};

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, WindowEvent};

use commands::AppState;
use settings::Settings;

const STARTED_HIDDEN_ARG: &str = "--hidden";
pub(crate) const TRAY_ID: &str = "main";

fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// Runs a command from the pack's git hooks and returns its exit code, or `None` to start the app.
pub fn run_hook_command() -> Option<i32> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    if !matches!(args.first(), Some(&(setup::MODSYNC_COMMAND | setup::IRIS_CLEAN_COMMAND))) {
        return None;
    }
    let dir = std::env::current_dir().unwrap_or_default();
    let log = jobs::Log::terminal();
    let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
    let result = runtime.block_on(async {
        match args[..] {
            [setup::IRIS_CLEAN_COMMAND] => iris_clean_stdio(),
            [_, "lock"] => modsync::lock(&dir, &log).await,
            [_, "adopt"] => modsync::adopt(&dir, &log).await.map(|_| ()),
            [_, "restore"] => modsync::restore(&dir, &log).await,
            [_, "status"] => modsync::status(&dir, &log),
            _ => Err(anyhow::anyhow!("usage: packraven {} lock|adopt|restore|status", setup::MODSYNC_COMMAND)),
        }
    });
    match result {
        Ok(()) => Some(0),
        Err(e) => {
            eprintln!("modsync: {e:#}");
            Some(1)
        }
    }
}

fn iris_clean_stdio() -> anyhow::Result<()> {
    use std::io::{Read, Write};
    let mut input = Vec::new();
    std::io::stdin().read_to_end(&mut input)?;
    std::io::stdout().write_all(modsync::iris_clean(&String::from_utf8_lossy(&input)).as_bytes())?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| show_main_window(app)))
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, Some(vec![STARTED_HIDDEN_ARG])))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let config_dir = app.path().app_config_dir()?;
            let settings_path = config_dir.join("settings.json");
            app.manage(AppState {
                settings: Mutex::new(Settings::load(&settings_path)),
                settings_path,
                data_dir: app.path().app_data_dir()?,
                jobs: jobs::Jobs::default(),
                test: Arc::new(testrun::TestRun::default()),
                watcher: Mutex::new(None),
                history: Mutex::default(),
                mod_list: Mutex::default(),
            });
            app.state::<AppState>().rewatch(app.handle());
            announcer::start(app.handle().clone());

            let show = MenuItem::with_id(app, "show", "Show packraven", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &quit])?;
            TrayIconBuilder::with_id(TRAY_ID)
                .icon(app.default_window_icon().cloned().expect("bundled icon"))
                .tooltip("packraven")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => show_main_window(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                        show_main_window(tray.app_handle());
                    }
                })
                .build(app)?;

            if !std::env::args().any(|a| a == STARTED_HIDDEN_ARG) {
                show_main_window(app.handle());
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let _ = window.hide();
                api.prevent_close();
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::save_settings,
            commands::overview,
            commands::get_pack_config,
            commands::save_pack_config,
            commands::set_version,
            commands::get_notes,
            commands::save_notes,
            commands::changelog_preview,
            commands::set_compare_baseline,
            commands::clear_compare_baseline,
            commands::track_directly,
            commands::revert_mods,
            git_commands::git_pull_preview,
            commands::server_sides,
            commands::set_side_override,
            mod_commands::installed_mods,
            mod_commands::check_mod_updates,
            mod_commands::mod_versions,
            mod_commands::set_update_channel,
            mod_commands::search_mods,
            mod_commands::mod_categories,
            mod_commands::preview_mods,
            mod_commands::change_mods,
            mod_commands::set_mod_locked,
            mod_commands::set_mod_enabled,
            mod_commands::mod_description,
            mod_commands::remove_mods,
            commands::publish_preflight,
            commands::build_client_export,
            commands::build_server_pack,
            commands::start_test,
            commands::start_client,
            commands::server_command,
            commands::kill_process,
            commands::publish,
            commands::releases,
            commands::output_dir,
            pack_commands::list_packs,
            pack_commands::discover_instances,
            pack_commands::inspect_instance,
            pack_commands::setup_instance,
            pack_commands::join_pack,
            pack_commands::switch_pack,
            pack_commands::remove_pack,
            pack_commands::update_hooks,
            pack_commands::get_pack_java,
            pack_commands::set_pack_java,
            git_commands::git_overview,
            git_commands::git_diff,
            git_commands::git_stage,
            git_commands::git_unstage,
            git_commands::git_discard,
            git_commands::git_ignore,
            git_commands::git_ignore_options,
            git_commands::git_ignore_preview,
            git_commands::git_untrack,
            git_commands::git_commit,
            git_commands::git_sync,
            file_commands::list_dir,
            file_commands::read_file,
            file_commands::write_file,
            file_commands::absolute_path,
            file_commands::create_file,
            file_commands::create_dir,
            file_commands::trash_paths,
            file_commands::copy_files,
            file_commands::paste_files,
            file_commands::rename_path,
            file_commands::move_paths,
            file_commands::open_pack_folder,
            file_commands::index_files,
            file_commands::copy_paths,
            file_commands::open_with_default_app,
            file_commands::default_app,
            file_commands::read_bytes,
            file_commands::read_nbt,
            file_commands::read_log,
            file_commands::decompile_class,
            file_commands::extract_archive,
            file_commands::compress_paths,
            file_commands::media_path,
            file_commands::import_paths,
            file_commands::drag_out,
            file_commands::file_history,
            file_commands::file_undo,
            system_commands::defender_status,
            system_commands::defender_exclude,
            system_commands::defender_dismiss,
            system_commands::git_version,
            system_commands::install_git,
            system_commands::credentials_status,
            system_commands::credential_store,
            system_commands::credential_use_env,
            system_commands::credential_clear,
            system_commands::github_identity,
            system_commands::env_var_names,
            system_commands::set_icon_theme,
            discord_commands::get_webhook,
            discord_commands::set_webhook,
            discord_commands::discord_test,
            discord_commands::announce_release,
            discord_commands::discohook_link,
            discord_commands::discohook_preview,
        ])
        .run(tauri::generate_context!())
        .expect("error while running packraven");
}
