// Claude — Date 11/03/2026
// No console window alongside the app on Windows, in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    drawbridge_app::run()
}
