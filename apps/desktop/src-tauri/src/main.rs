// Sin consola en Windows en release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    mythos_desktop_lib::run()
}
