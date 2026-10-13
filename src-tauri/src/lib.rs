mod announcer;
mod changelog;
mod commands;
mod curseforge;
mod defender;
mod discord;
mod discord_commands;
mod export;
mod file_commands;
mod files;
mod git;
mod git_commands;
mod gitops;
mod http;
mod jobs;
mod launch;
mod modrinth;
mod pack;
mod pack_commands;
mod process;
mod publish;
mod settings;
mod setup;
mod sides;
mod sync;
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

fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
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
            });
            app.state::<AppState>().rewatch(app.handle());
            announcer::start(app.handle().clone());

            let show = MenuItem::with_id(app, "show", "Show packraven", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &quit])?;
            TrayIconBuilder::new()
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
            commands::server_sides,
            commands::set_side_override,
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
            pack_commands::switch_pack,
            pack_commands::remove_pack,
            pack_commands::update_hooks,
            git_commands::git_overview,
            git_commands::git_diff,
            git_commands::git_stage,
            git_commands::git_unstage,
            git_commands::git_discard,
            git_commands::git_commit,
            git_commands::git_sync,
            file_commands::list_dir,
            file_commands::read_file,
            file_commands::write_file,
            file_commands::absolute_path,
            system_commands::defender_status,
            system_commands::defender_exclude,
            system_commands::defender_dismiss,
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
