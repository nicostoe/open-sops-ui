// No console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod ui_test;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use open_sops_ui_core::App;
use serde_json::Value;
use tauri::Manager;
use tauri_plugin_clipboard_manager::ClipboardExt;

struct Shared(Mutex<App>);

/// Tracks the secret the app last put on the clipboard, so it can be removed
/// again: after the time set in the settings, or when the app closes.
#[derive(Default)]
struct Clipboard {
    /// Counts copies; a timer only acts if nothing was copied after it started.
    generation: AtomicU64,
    pending: Mutex<Option<String>>,
}

/// Removes `secret` from the clipboard, but only if it is still there.
/// Whatever the user copied elsewhere in the meantime is left alone.
///
/// Clipboard managers often restore the previous content when the clipboard
/// becomes empty, which would bring the secret straight back. So after
/// clearing, check again, and if it returned, replace it with a visible note
/// instead: a manager keeps that as the newest entry.
fn clear_if_still_there(app: &tauri::AppHandle, secret: &str) {
    let holds_secret = || app.clipboard().read_text().ok().as_deref() == Some(secret);
    if !holds_secret() {
        return;
    }
    let _ = app.clipboard().clear();
    std::thread::sleep(Duration::from_millis(600));
    if holds_secret() {
        let _ = app.clipboard().write_text(CLEARED_NOTE.to_string());
    }
}

/// What the clipboard holds after a secret was removed and a clipboard
/// manager wouldn't let it stay empty.
const CLEARED_NOTE: &str = "(cleared by Open SOPS UI)";

/// Copies text to the clipboard. The page has no clipboard access of its own;
/// everything goes through here. A `secret` is removed again after the
/// configured time. Returns that time in seconds, 0 if it stays.
///
/// With `machine_key` set, it copies the private key of the key pair made for a
/// server or CI instead of `text`: that key goes from the backend straight to
/// the clipboard and never passes through the page.
#[tauri::command(async)]
fn copy(
    app: tauri::AppHandle,
    shared: tauri::State<'_, Shared>,
    clipboard: tauri::State<'_, Clipboard>,
    text: Option<String>,
    secret: bool,
    machine_key: Option<bool>,
) -> Result<u32, String> {
    let text = if machine_key == Some(true) {
        shared
            .0
            .lock()
            .map_err(|_| "The app hit an internal error. Restart it.".to_string())?
            .machine_key_secret()
            .ok_or("That key is gone. Create a new one.")?
    } else {
        text.ok_or("Nothing to copy.")?
    };
    let secret = secret || machine_key == Some(true);
    let failed = |_| "Couldn't copy to the clipboard.".to_string();
    app.clipboard().write_text(text.clone()).map_err(failed)?;
    let seconds = if secret {
        shared
            .0
            .lock()
            .map(|a| a.clipboard_clear_seconds())
            .unwrap_or(0)
    } else {
        0
    };
    let generation = clipboard.generation.fetch_add(1, Ordering::SeqCst) + 1;
    *clipboard
        .pending
        .lock()
        .map_err(|_| "The app hit an internal error. Restart it.".to_string())? =
        (seconds > 0).then(|| text.clone());
    if seconds > 0 {
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(seconds.into()));
            let state = app.state::<Clipboard>();
            if state.generation.load(Ordering::SeqCst) == generation {
                clear_if_still_there(&app, &text);
                if let Ok(mut pending) = state.pending.lock() {
                    *pending = None;
                }
            }
        });
    }
    Ok(seconds)
}

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
///
/// Only folders that belong to the app itself: the executable's folder and the
/// bundle's resources. The folder an AppImage file sits in is deliberately not
/// one of them. AppImages are often started from Downloads, and a stray file
/// named `sops` there would otherwise run with the user's keys. Inside a running
/// AppImage, the executable's folder is its own read-only mount.
fn bundled_dirs(exe: &Path, resource_dir: Option<PathBuf>) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    let candidates = [exe.parent().map(PathBuf::from), resource_dir];
    for dir in candidates.into_iter().flatten() {
        if !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    dirs
}

/// `open-sops-ui --self-check [file]` reports, without opening a window, which
/// sops the installed app would use. CI runs it against the built packages.
/// The report also goes to `file`, because a Windows GUI binary has no console.
fn self_check(bundled: &[PathBuf], version: &str, out_file: Option<&String>) -> ! {
    let report = open_sops_ui_core::self_check(bundled, version);
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

// The page never passes a URL to open, only a release tag or the name of one of
// these pages, so it can't send the browser anywhere but this project.
const REPO_URL: &str = "https://github.com/nicostoe/open-sops-ui";
const RELEASES_URL: &str = "https://github.com/nicostoe/open-sops-ui/releases";

fn open_in_browser(app: &tauri::AppHandle, url: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    // The UI self-test notes the address instead of starting a browser.
    if ui_test::record_opened_page(&url) {
        return Ok(());
    }
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|_| "Couldn't open the browser.".to_string())
}

/// The page of a release. Development builds (`x.y.z-dev.N`) have none, so
/// they get the list of releases.
fn release_url(tag: &str) -> Option<String> {
    if !open_sops_ui_core::valid_release_tag(tag) {
        return None;
    }
    Some(if tag.contains("-dev.") {
        RELEASES_URL.to_string()
    } else {
        format!("{RELEASES_URL}/tag/{tag}")
    })
}

/// Fixed pages of the project: its repository and where to report a problem.
fn project_url(page: &str) -> Option<String> {
    match page {
        "repo" => Some(REPO_URL.to_string()),
        "issues" => Some(format!("{REPO_URL}/issues")),
        _ => None,
    }
}

#[tauri::command]
fn open_release_page(app: tauri::AppHandle, tag: String) -> Result<(), String> {
    let url = release_url(&tag).ok_or("That isn't a release.")?;
    open_in_browser(&app, url)
}

#[tauri::command]
fn open_project_page(app: tauri::AppHandle, page: String) -> Result<(), String> {
    let url = project_url(&page).ok_or("That page doesn't exist.")?;
    open_in_browser(&app, url)
}

/// The version of this build: the release tag, compiled in by CI (see
/// scripts/version.sh). Local builds fall back to the version in tauri.conf.json.
fn build_version(context: &tauri::Context) -> String {
    option_env!("OPEN_SOPS_UI_VERSION")
        .filter(|v| !v.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| context.package_info().version.to_string())
}

fn main() {
    let context = tauri::generate_context!();
    let version = build_version(&context);
    let exe = std::env::current_exe().expect("the app's own path is unknown");
    let resource_dir =
        tauri::utils::platform::resource_dir(context.package_info(), &tauri::Env::default()).ok();
    let bundled = bundled_dirs(&exe, resource_dir);

    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--self-check") {
        self_check(&bundled, &version, args.get(1));
    }
    let folder = args
        .first()
        .and_then(|arg| std::path::absolute(arg).ok())
        .filter(|p| p.is_dir());

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_opener::init())
        .manage(Clipboard::default())
        .setup(move |app| {
            let mut core = App::new(folder, bundled);
            core.set_version(&version);
            app.manage(Shared(Mutex::new(core)));
            if ui_test::active() {
                if let Some(window) = app.get_webview_window("main") {
                    window.set_title(ui_test::WINDOW_TITLE)?;
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            api,
            copy,
            open_release_page,
            open_project_page,
            ui_test::ui_test
        ])
        .build(context)
        .expect("failed to start open-sops-ui")
        .run(|app, event| {
            // Don't leave a copied secret behind when the app closes before its timer ran.
            if let tauri::RunEvent::Exit = event {
                let pending = app
                    .state::<Clipboard>()
                    .pending
                    .lock()
                    .ok()
                    .and_then(|mut p| p.take());
                if let Some(secret) = pending {
                    clear_if_still_there(app, &secret);
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_this_projects_pages_open() {
        assert_eq!(project_url("repo").as_deref(), Some(REPO_URL));
        assert_eq!(
            project_url("issues").as_deref(),
            Some("https://github.com/nicostoe/open-sops-ui/issues")
        );
        assert_eq!(project_url("https://evil.example"), None);
        assert_eq!(
            release_url("v0.1.0-beta.5").as_deref(),
            Some("https://github.com/nicostoe/open-sops-ui/releases/tag/v0.1.0-beta.5")
        );
        assert_eq!(
            release_url("v0.1.0-dev.57").as_deref(),
            Some(RELEASES_URL),
            "dev builds have no release page"
        );
        assert_eq!(release_url("v1.0.0/../../evil"), None);
    }

    /// A sops lying next to an AppImage file must never be picked up.
    #[test]
    fn the_folder_an_appimage_sits_in_is_not_searched() {
        let downloads = std::env::temp_dir().join("open-sops-ui-downloads");
        std::env::set_var("APPIMAGE", downloads.join("Open-SOPS-UI.AppImage"));
        let exe = Path::new("/tmp/.mount_OpenSOabc/usr/bin/open-sops-ui");
        let resources = PathBuf::from("/tmp/.mount_OpenSOabc/usr/lib/Open SOPS UI");
        let dirs = bundled_dirs(exe, Some(resources.clone()));
        assert_eq!(
            dirs,
            vec![PathBuf::from("/tmp/.mount_OpenSOabc/usr/bin"), resources]
        );
        assert!(!dirs.contains(&downloads));
    }
}
