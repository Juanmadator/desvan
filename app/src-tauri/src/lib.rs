//! Backend de la app: el árbol vive en Rust y la interfaz le pide solo lo que muestra.

mod commands;
mod state;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(state::AppState::default())
        .invoke_handler(tauri::generate_handler![
            commands::startup_args,
            commands::volumes,
            commands::elevation,
            commands::relaunch_admin,
            commands::scan_start,
            commands::scan_cancel,
            commands::scan_summary,
            commands::node,
            commands::children,
            commands::ancestors,
            commands::search,
            commands::largest,
            commands::ext_stats,
            commands::layout,
            commands::junk,
            commands::dupes_start,
            commands::dupes_cancel,
            commands::delete,
            commands::reveal,
            commands::open_path,
        ])
        .run(tauri::generate_context!())
        .expect("error al arrancar Desván");
}
