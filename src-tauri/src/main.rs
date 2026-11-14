// No console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::Mutex;

use serde_json::Value;
use sops_env_core::App;
use tauri::Manager;

struct Shared(Mutex<App>);

/// The single bridge between the UI and the backend. `async` moves the call
/// off the main thread, so the window stays responsive while sops runs.
#[tauri::command(async)]
fn api(state: tauri::State<'_, Shared>, action: String, body: Value) -> Result<Value, String> {
    let mut app = state.0.lock().map_err(|_| "The app hit an internal error. Restart it.".to_string())?;
    app.handle(&action, &body)
}

fn main() {
    // `sops edit` starts this same binary as its editor; see the core crate.
    if let Some(code) = sops_env_core::run_as_editor_if_requested() {
        std::process::exit(code);
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .setup(|app| {
            let exe = std::env::current_exe()?;
            // Folders searched for a shipped sops binary, most specific first.
            let mut bundled = Vec::new();
            let mut add = |dir: Option<PathBuf>| {
                if let Some(d) = dir.filter(|d| !bundled.contains(d)) {
                    bundled.push(d);
                }
            };
            // An AppImage runs from a temporary mount; look beside the AppImage file itself.
            add(std::env::var_os("APPIMAGE").map(PathBuf::from).and_then(|p| p.parent().map(PathBuf::from)));
            add(exe.parent().map(PathBuf::from));
            add(app.path().resource_dir().ok());

            let folder = std::env::args()
                .nth(1)
                .and_then(|arg| std::path::absolute(arg).ok())
                .filter(|p| p.is_dir());
            app.manage(Shared(Mutex::new(App::new(folder, bundled, exe))));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![api])
        .run(tauri::generate_context!())
        .expect("failed to start sops-env");
}
