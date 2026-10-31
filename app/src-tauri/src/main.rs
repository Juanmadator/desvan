// Sin consola en las compilaciones de publicación
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // modo ayudante: lo lanza la propia app, elevado, para leer la MFT
    let args: Vec<String> = std::env::args().collect();
    #[cfg(windows)]
    if args.len() >= 4 && args[1] == "--mft-helper" {
        let letter = args[2].chars().next().unwrap_or('C');
        std::process::exit(desvan_core::helper::serve(letter, &args[3]));
    }
    let _ = args;
    desvan_app_lib::run()
}
