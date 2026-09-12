//! `desktop` binary entry point; all real logic lives in `desktop_lib` (`src/lib.rs`).
// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    desktop_lib::run()
}
