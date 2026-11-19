//! Support for the scripted UI self-test.
//!
//! When the app is started with `OPEN_SOPS_UI_SELFTEST=<folder>`, the frontend
//! loads `ui/selftest.js`, which clicks through the app in its real window.
//! That works on every platform, including macOS, where no WebDriver exists
//! for the system web view. This module gives the script what the page can't
//! do itself: take screenshots, write the result and exit.
//!
//! Without the variable every call is a no-op.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{json, Value};

/// The window title in test mode. Screenshots find the window by it, so it
/// must never match a copy of the app the user has open on the same desktop.
pub const WINDOW_TITLE: &str = "Open SOPS UI self-test";

/// True when the app runs its scripted self-test.
pub fn active() -> bool {
    output_dir().is_some()
}

fn output_dir() -> Option<PathBuf> {
    std::env::var_os("OPEN_SOPS_UI_SELFTEST")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// Captures the app as the user would see it. Linux grabs only the app's
/// window; macOS and Windows have no simple way to do that and grab the
/// screen, which on a CI runner shows nothing else.
fn screenshot(file: &Path) -> bool {
    let path = file.to_string_lossy();
    let mut cmd = if cfg!(target_os = "macos") {
        let mut c = Command::new("screencapture");
        c.args(["-x", &path]);
        c
    } else if cfg!(windows) {
        let script = format!(
            "Add-Type -AssemblyName System.Windows.Forms,System.Drawing; \
             $b = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds; \
             $bmp = New-Object System.Drawing.Bitmap $b.Width, $b.Height; \
             $g = [System.Drawing.Graphics]::FromImage($bmp); \
             $g.CopyFromScreen($b.Location, [System.Drawing.Point]::Empty, $b.Size); \
             $bmp.Save('{}')",
            path.replace('\'', "''")
        );
        let mut c = Command::new("powershell");
        c.args(["-NoProfile", "-NonInteractive", "-Command", &script]);
        c
    } else {
        // ImageMagick; needs an X11 display (Xvfb in CI).
        let mut c = Command::new("import");
        c.args(["-window", WINDOW_TITLE, &path]);
        c
    };
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    cmd.status().map(|s| s.success()).unwrap_or(false) && file.is_file()
}

/// Commits one file, so the test can show what "changed since the last
/// commit" looks like. The app itself never commits anything.
fn commit(file: &Path) -> bool {
    let Some(dir) = file.parent() else {
        return false;
    };
    let git = |args: &[&str]| {
        let mut cmd = Command::new("git");
        cmd.args([
            "-c",
            "user.name=UI test",
            "-c",
            "user.email=ui-test@example.invalid",
        ])
        .args(["-c", "commit.gpgsign=false", "-c", "core.autocrlf=false"])
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000);
        }
        cmd.status().map(|s| s.success()).unwrap_or(false)
    };
    let name = file
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    git(&["add", "--", &name]) && git(&["commit", "-q", "-m", "Encrypt the env file", "--", &name])
}

/// In test mode, notes which address would open instead of starting a browser
/// on the CI runner. Returns false outside test mode.
pub fn record_opened_page(url: &str) -> bool {
    match output_dir() {
        Some(dir) => std::fs::write(dir.join("opened-page.txt"), url).is_ok(),
        None => false,
    }
}

#[tauri::command(async)]
pub fn ui_test(app: tauri::AppHandle, action: String, body: Value) -> Result<Value, String> {
    let Some(dir) = output_dir() else {
        return Ok(Value::Null);
    };
    match action.as_str() {
        "config" => Ok(json!({ "enabled": true })),
        // A fixed release list instead of asking GitHub, written by ui-tests/selftest.mjs,
        // or an object asking the page to simulate a failure ({"simulate": "error" | "hang"}).
        "releases" => {
            let text =
                std::fs::read_to_string(dir.join("releases.json")).map_err(|e| e.to_string())?;
            serde_json::from_str(&text).map_err(|e| e.to_string())
        }
        "setReleases" => {
            let text = serde_json::to_string(&body["releases"]).unwrap_or_else(|_| "[]".into());
            Ok(json!(
                std::fs::write(dir.join("releases.json"), text).is_ok()
            ))
        }
        // Which address the app would have opened; see `record_opened_page`.
        "openedPage" => Ok(json!(
            std::fs::read_to_string(dir.join("opened-page.txt")).ok()
        )),
        "shot" => {
            let name: String = body["name"]
                .as_str()
                .unwrap_or("shot")
                .chars()
                .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
                .collect();
            Ok(json!(screenshot(&dir.join(format!("{name}.png")))))
        }
        // What is on the clipboard right now, to check that copied values get cleared.
        "clipboard" => {
            use tauri_plugin_clipboard_manager::ClipboardExt;
            Ok(json!(app.clipboard().read_text().unwrap_or_default()))
        }
        // Changes a file behind the app's back, like a git pull would.
        "writeFile" => {
            let path = body["path"].as_str().unwrap_or("");
            Ok(json!(std::fs::write(
                path,
                body["text"].as_str().unwrap_or("")
            )
            .is_ok()))
        }
        "commit" => Ok(json!(commit(Path::new(
            body["path"].as_str().unwrap_or("")
        )))),
        "done" => {
            let text = serde_json::to_string_pretty(&body).unwrap_or_default();
            let written = std::fs::write(dir.join("result.json"), text).is_ok();
            app.exit(if written && body["ok"] == true { 0 } else { 1 });
            Ok(Value::Null)
        }
        _ => Err("Unknown test action.".into()),
    }
}
