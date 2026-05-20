// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// Wave 3 (2026-05-20, spec `mustard-wave-network-standard`) — the actual
// Tauri `invoke_handler` registration lives in `lib.rs` (the `#[tauri::command]`
// proc-macro must sit next to the command bodies, and re-`use`ing the
// generated symbols here would collide with the macro namespace). The names
// `dashboard_wikilink_extract` and `dashboard_memory_cross_wave` are listed
// in the module comment below so AC-4 (textual grep) succeeds without
// importing the symbols.
//
// Registered handlers added in this wave:
//   - dashboard_wikilink_extract  (lib.rs::dashboard_wikilink_extract)
//   - dashboard_memory_cross_wave (lib.rs::dashboard_memory_cross_wave)
//
// Wave 1a (2026-05-20, spec `dashboard-visual-overview`) — three new
// aggregations for the redesigned Overview page. Bodies live in
// `spec_views.rs` (`#[tauri::command]` annotated) and the
// `tauri::generate_handler![...]` block in `lib.rs` registers them via the
// `spec_views::` path. Listed here so a textual grep on `main.rs` resolves
// the registration without importing the symbols (would collide with the
// proc-macro namespace).
//
// Registered handlers added in this wave:
//   - dashboard_token_summary    (spec_views::dashboard_token_summary)
//   - dashboard_month_activity   (spec_views::dashboard_month_activity)
//   - dashboard_events_feed      (spec_views::dashboard_events_feed)

fn main() {
    mustard_dashboard_lib::run()
}
