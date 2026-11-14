// No console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod ui_test;

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde_json::Value;
use sops_env_core::App;
use tauri::Manager;

struct Shared(Mutex<App>);

/// The single bridge between the UI and the backend. `async` moves the call
/// off the main thread, so the window stays responsive while sops runs.
#[tauri::command(async)]
fn api(state: tauri::State<'_, Shared>, action: String, body: Value) -> Result<Value, String> {
    let mut app = state
        .0
        .lock()
        .map_err(|_| "The app hit an internal error. Restart it.".to_string())?;
    app.handle(&action, &body)
}

/// Folders searched for a shipped sops binary, most specific first.
fn bundled_dirs(exe: &Path, resource_dir: Option<PathBuf>) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    let candidates = [
        // An AppImage runs from a temporary mount; look beside the AppImage file itself.
        std::env::var_os("APPIMAGE")
            .map(PathBuf::from)
            .and_then(|p| p.parent().map(PathBuf::from)),
        exe.parent().map(PathBuf::from),
        resource_dir,
    ];
    for dir in candidates.into_iter().flatten() {
        if !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    dirs
}

/// `sops-env --self-check [file]` reports, without opening a window, which
/// sops the installed app would use. CI runs it against the built packages.
/// The report also goes to `file`, because a Windows GUI binary has no console.
fn self_check(bundled: &[PathBuf], version: &str, out_file: Option<&String>) -> ! {
    let report = sops_env_core::self_check(bundled, version);
    let text = serde_json::to_string_pretty(&report).unwrap_or_default();
    if let Some(path) = out_file {
        if std::fs::write(path, &text).is_err() {
            std::process::exit(2);
        }
    }
    println!("{text}");
    std::process::exit(if report["sops"]["path"].is_null() {
        1
    } else {
        0
    });
}

fn main() {
    // `sops edit` starts this same binary as its editor; see the core crate.
    if let Some(code) = sops_env_core::run_as_editor_if_requested() {
        std::process::exit(code);
    }

    let context = tauri::generate_context!();
    let exe = std::env::current_exe().expect("the app's own path is unknown");
    let resource_dir =
        tauri::utils::platform::resource_dir(context.package_info(), &tauri::Env::default()).ok();
    let bundled = bundled_dirs(&exe, resource_dir);

    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--self-check") {
        self_check(
            &bundled,
            &context.package_info().version.to_string(),
            args.get(1),
        );
    }
    let folder = args
        .first()
        .and_then(|arg| std::path::absolute(arg).ok())
        .filter(|p| p.is_dir());

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .setup(move |app| {
            app.manage(Shared(Mutex::new(App::new(folder, bundled, exe))));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![api, ui_test::ui_test])
        .run(context)
        .expect("failed to start sops-env");
}
