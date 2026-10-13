// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
  if let Some(code) = packraven_lib::run_hook_command() {
    std::process::exit(code);
  }
  packraven_lib::run();
}
