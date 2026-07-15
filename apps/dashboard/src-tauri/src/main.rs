// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// The Tauri `invoke_handler` registration lives in `lib.rs` — the
// `#[tauri::command]` proc-macro must sit next to the command bodies, and
// re-`use`ing the generated symbols here would collide with the macro
// namespace.

fn main() {
    mustard_dashboard_lib::run()
}
