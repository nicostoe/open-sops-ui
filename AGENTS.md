# Agent guide for Sops Env

A Tauri 2 desktop app for SOPS-encrypted env files. See [README.md](README.md) for what it does and how to build it.

## Layout

- `core/`: the `sops-env-core` crate. All backend logic: keys, env parsing, `.sops.yaml` rules, calls to `sops`. No GUI dependencies, so it compiles and tests without WebKit.
  - `src/lib.rs`: `App::handle(action, body)` is the single entry point the UI calls.
  - `tests/flow.rs`: end-to-end test against a real `sops`.
  - `src/bin/edit-helper.rs`: test-only stand-in for the app's editor mode.
- `src-tauri/`: the desktop shell. `src/main.rs` wires one `api` command to `App::handle`. `tauri.conf.json` holds window, CSP and bundle settings; `capabilities/default.json` the permissions.
- `ui/`: the frontend. `index.html`, `style.css`, `app.js`. Vanilla JS, no bundler, no dependencies.
- `.github/workflows/build.yml`: tests the backend and builds packages on Linux, Windows and macOS runners. It uploads artifacts only; it doesn't publish releases.
- `app-icon.svg`: icon source. Regenerate with `npx tauri icon app-icon.svg`, then delete `src-tauri/icons/android` and `ios`.

## Commands

```bash
cargo test -p sops-env-core
```

```bash
npm run dev
```

```bash
npm run build
```

## How it works

- **UI to backend:** `app.js` calls `window.__TAURI__.core.invoke("api", { action, body })`. Adding a feature means adding a match arm in `App::handle` and calling it from `app.js`; no new Tauri command or permission is needed.
- **sops lookup:** a `sops` / `sops.exe` next to the app (or next to the AppImage, or in the resource folder) wins over `PATH`. `find_program` also checks Homebrew and user-local folders, because apps started from a launcher get a short `PATH`.
- **Keys:** all loaded age identities are joined and passed to sops per call through `SOPS_AGE_KEY`. Public keys come from the `age` crate.
- **Saving an encrypted file:** runs `sops edit` with this app's own binary as the editor. `main` calls `run_as_editor_if_requested` first; in that mode the process fetches the plaintext from a one-shot loopback socket (address and token in env vars) and writes it to sops's temp file. This keeps unchanged values byte-identical.
- **Encrypting plaintext or changing recipients:** pipes plaintext to `sops encrypt` on stdin.
- **Working directory:** sops finds `.sops.yaml` upward from its working directory and fails if one exists without a matching rule. Calls with explicit recipients run from `neutral_dir()`; only rule-based encryption runs from the file's folder.
- **Format:** always pass `--input-type dotenv --output-type dotenv`; names like `.env.prod.enc` aren't detected by extension.

## Rules

- Never write secret keys or decrypted values to disk, logs, the config file or anything returned to the UI. The only exception is the explicit "Save to default location" action, which appends to `keys.txt` and never overwrites it.
- Must work on Linux, Windows and macOS. Use `std::path`, guard Unix-only code with `cfg(unix)`, keep `CREATE_NO_WINDOW` on child processes, and don't assume a shell.
- Keep the CSP strict: no inline scripts, no remote resources. Inline `style` attributes are allowed.
- Use in-app dialogs (`ask`, `show`) instead of `confirm()` or `alert()`; those are unreliable in system web views. File and folder pickers go through the dialog plugin, clipboard through the clipboard plugin.
- Every color is a CSS variable with a light and a dark value.
- UI copy: sentence case, no "please" or "successfully", errors say what happened and what to do.

## Testing

- Backend: `cargo test -p sops-env-core`. Set `SOPS_ENV_TEST_EDITOR` to the built desktop binary to run the same flow through its editor mode.
- The test sets `XDG_CONFIG_HOME` to a temp folder. Do the same when running the app by hand, so the real `keys.txt` and config are never touched.
- There are no automated UI tests. Windows and macOS builds have not been run.
