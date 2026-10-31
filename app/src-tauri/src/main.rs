// Sin consola en las compilaciones de publicación
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    desvan_app_lib::run()
}
