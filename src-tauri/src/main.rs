// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Before anything else: this same executable doubles as ssh's askpass
    // helper. See `services::tunnel` for why, and why the check is this
    // strict.
    if rezureapp_lib::services::tunnel::serve_askpass() {
        return;
    }
    rezureapp_lib::run();
}
