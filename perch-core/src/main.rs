#![windows_subsystem = "windows"]

use std::env;

mod util;
mod todo;
mod reminder;
mod weather;
mod battery;
mod wallpaper;
mod media;
mod water;
mod theme;
mod cli;

fn main() {
    let args: Vec<String> = env::args().collect();
    util::init_language();
    if let Err(e) = cli::run_cli(args) {
        eprintln!("[ERROR] {}", e);
    }
}
