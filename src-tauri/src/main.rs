#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if let Some(code) = lantern_lib::askpass_exit_code() {
        std::process::exit(code);
    }
    lantern_lib::run();
}
