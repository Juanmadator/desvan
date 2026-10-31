//! Backend de la app: el árbol vive en Rust y la interfaz le pide solo lo que muestra.

mod bridge;
mod commands;
mod state;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(state::AppState::default())
        .setup(|app| {
            // la ventana se crea aquí para poder cambiar la carpeta de datos de WebView2
            // (las pruebas automáticas usan una propia y no chocan con la app instalada)
            let mut b = tauri::WebviewWindowBuilder::new(app, "main", tauri::WebviewUrl::default())
                .title("Desván")
                .inner_size(1440.0, 900.0)
                .min_inner_size(960.0, 600.0)
                .background_color(tauri::window::Color(13, 15, 20, 255))
                .disable_drag_drop_handler();
            if let Ok(dir) = std::env::var("DESVAN_WEBVIEW_DATA") {
                b = b.data_directory(dir.into());
            }
            b.build()?;
            Ok(())
        })
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
            commands::undo_delete,
            commands::reveal,
            commands::open_path,
        ])
        .run(tauri::generate_context!())
        .expect("error al arrancar Desván");
}
