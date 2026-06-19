// Prevents additional console window on Windows release
// #![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    kalshi_monster_lib::run();
}
