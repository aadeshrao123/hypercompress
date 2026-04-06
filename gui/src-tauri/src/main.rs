#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Always launch the GUI. CLI args (from context menu) are passed
    // to the frontend via a Tauri command so the GUI opens with the
    // file pre-loaded on the right tab.
    gui_lib::run()
}
