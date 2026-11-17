# Agent guide for Open SOPS UI

A Tauri 2 desktop app for SOPS-encrypted env files. See [README.md](README.md) for what it does and how to build it.

## Layout

- `core/`: the `open-sops-ui-core` crate. All backend logic: keys, env parsing, `.sops.yaml` rules, calls to `sops`. No GUI dependencies, so it compiles and tests without WebKit.
  - `src/lib.rs`: `App::handle(action, body)` is the single entry point the UI calls.
  - `tests/flow.rs`: end-to-end test against a real `sops`.
- `src-tauri/`: the desktop shell. `src/main.rs` wires one `api` command to `App::handle`. `tauri.conf.json` holds window, CSP and bundle settings; `capabilities/default.json` the permissions.
- `ui/`: the frontend. `index.html`, `style.css`, `app.js`. Vanilla JS, no bundler, no dependencies.
- `ui/selftest.js` and `src-tauri/src/ui_test.rs`: the scripted UI self-test. With `OPEN_SOPS_UI_SELFTEST=<folder>` set, the app clicks through itself, takes screenshots and exits. Inactive otherwise.
- `ui-tests/selftest.mjs`: runs that test against a binary with throwaway folders and checks the files on disk. Plain Node, no dependencies.
- `.github/workflows/build.yml`: on pull requests, pushes to `main`, weekly, and called by the release workflow. Jobs: `build` (tests, packages, smoke test and UI test per platform), `distros` (packages installed and UI-tested in distribution containers), `os-versions` (UI test on the older macOS runners), `screenshots` (one artifact with every platform's pictures).
- `.github/workflows/release.yml`: on `v*` tags. Checks the tag against `tauri.conf.json`, calls `build.yml`, then publishes a GitHub release. A tag with a `-suffix` becomes a pre-release.
- `.github/workflows/audit.yml`: weekly `cargo audit` and `npm audit`.
- `scripts/fetch-sops.sh`: downloads the pinned sops into `vendor/sops/` (git-ignored) and verifies it with cosign and SHA-256. The `SOPS_VERSION` line carries a Renovate comment; keep its format, `renovate.json` matches it with a regex.
- `scripts/smoke-test.sh`: installs the built package and runs `open-sops-ui --self-check` (CI only; it installs the app).
- `scripts/ui-test.sh`, `scripts/distro-test.sh`: run the UI test, on a runner or inside a distribution container.
- `cliff.toml`, `scripts/release-notes.sh`: release notes from commit messages. The release workflow attaches them to each release; every CI run on Linux shows the preview for the next one on its summary page.
- `scripts/collect-bundles.sh`, `scripts/ci-annotate.sh`: CI helpers for package names and public failure annotations.
- `app-icon.svg`: icon source. Regenerate with `npx tauri icon app-icon.svg`, then delete `src-tauri/icons/android` and `ios`.

## Commands

```bash
cargo test -p open-sops-ui-core
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
- **Saving an encrypted file:** compares the UI's variables with the decrypted file and applies each difference with `sops set --value-stdin` or `sops unset`. Values travel through stdin, so no decrypted content reaches the disk (`sops edit` writes a temp file, which is why it isn't used), and untouched variables keep their ciphertext. If a step fails, the original bytes are written back. Names that can't be addressed as `['NAME']` fall back to re-encrypting the whole file for its current recipients.
- **Refreshing:** the UI reads the state again on the refresh button and whenever the window regains focus (at most every two seconds), so files added by a `git pull` appear. No file watcher: it would cost resources and behave differently per OS. Each listed file carries its `rev`; when the open file's differs, it reloads silently without unsaved edits, or keeps them and shows a note.
- **Changed on disk:** `open` returns a fingerprint (`rev`) of the file; `save` sends it back and refuses with `{conflict: true}` when the file changed meanwhile, until the user confirms (`overwrite`).
- **Encodings:** `read_text` decodes UTF-8 (with or without BOM), UTF-16 and Windows-1252 and rejects files that aren't text or are over 2 MB. Use it for every env or key file; `read_to_string` is only for files the app or git wrote.
- **Clipboard:** the page has no clipboard permission. Copying goes through the `copy` command in `main.rs`, which removes a secret again after the configured time and when the app exits. Clipboard managers restore emptied clipboards, so after clearing it checks and, if the secret came back, replaces it with a short note.
- **Encrypting plaintext or changing recipients:** pipes plaintext to `sops encrypt` on stdin.
- **Working directory:** sops finds `.sops.yaml` upward from its working directory and fails if one exists without a matching rule. Calls with explicit recipients run from `neutral_dir()`; only rule-based encryption runs from the file's folder.
- **Git safety:** every git call goes through `git_read`, which switches off the programs a repository's own config can make git run (`core.fsmonitor`, hooks). Opening a folder must never execute anything from it; `untrusted_repository_config_is_not_executed` guards this. Never call `run(git, …)` directly.
- **Git:** optional everywhere. Repository and branch come from the file system (`repo_root`, `repo_branch` read `.git`), so grouping works without the `git` binary. Status needs `git` and costs two calls per repository (`git_states`), never one per file. `changes` decrypts `git show HEAD:<file>` and compares by variable. The app reads from Git and may append to `.gitignore`; it never commits, pushes, or writes decrypted values to disk.
- **Format:** always pass `--input-type dotenv --output-type dotenv`; names like `.env.prod.enc` aren't detected by extension.

## Commit messages

Release notes are generated from commit subjects (`cliff.toml`), so every commit uses [Conventional Commits](https://www.conventionalcommits.org): `type(scope): description`, imperative, lower case after the colon. The type decides where it shows up:

| Type | Section in the release notes |
| --- | --- |
| `feat` | Features |
| `fix` | Bug fixes |
| `fix(security)`, `feat(security)` | Security |
| `perf` | Performance |
| `…(deps)` | Dependencies (Renovate writes these) |
| `docs`, `test`, `ci`, `build`, `chore`, `refactor`, `style` | left out |

Write the description for someone using the app ("fix: keep the scroll position after saving"), not about the code. Mark a breaking change with `!` (`feat!: …`). A commit that doesn't follow the format still appears, under "Other changes". Preview the notes with `scripts/release-notes.sh` (needs `cargo install git-cliff --locked`).

## Rules

- Never use `sops edit`, and never hand decrypted content to sops in a file or an argument: only through stdin.
- Never write secret keys or decrypted values to disk, logs, the config file or anything returned to the UI. The only exception is the explicit "Save to default location" action, which appends to `keys.txt` (`append_private`) and never rewrites what is already in it.
- Git must stay optional: every feature has to work in a folder without a repository and on a machine without `git`. Add Git information as extra fields that may be null, and keep `works_without_git_installed` passing.
- Must work on Linux, Windows and macOS. Use `std::path`, guard Unix-only code with `cfg(unix)`, keep `CREATE_NO_WINDOW` on child processes, and don't assume a shell.
- Keep the CSP strict: no inline scripts, no remote resources. Inline `style` attributes are allowed.
- Use in-app dialogs (`ask`, `show`) instead of `confirm()` or `alert()`; those are unreliable in system web views. File and folder pickers go through the dialog plugin.
- Every color is a CSS variable with a light and a dark value.
- UI copy: sentence case, no "please" or "successfully", errors say what happened and what to do.

## Testing

The project is kept up to date by Renovate with auto-merge, so the tests are the safety net. Every change needs a test that would fail without it.

- **Git:** `git_flow` in `tests/flow.rs` covers status, comparison and `.gitignore` against a real repository; `works_without_git_installed` in `lib.rs` covers the app with no `git` binary. The UI test's project is a repository, and its test-only `commit` action (in `ui_test.rs`) creates the commit to compare against.
- **Backend:** `cargo test -p open-sops-ui-core`. `tests/flow.rs` is one end-to-end flow against a real sops; extend it when you add or change an action. The test sets `EDITOR` to a program that doesn't exist, so a save that tried to use an editor would fail.
- **UI:** `node ui-tests/selftest.mjs target/release/open-sops-ui`. The steps live in `ui/selftest.js`; extend them when you change `ui/` or add a flow, add a `shot(...)` for every new screen, and update `EXPECTED` and `EXPECTED_SHOTS` in `ui-tests/selftest.mjs`. Native file dialogs can't be driven; the test passes the folder as an argument instead. Wait for a specific element, never for text that is also in a static hint. A closed dialog keeps its old content, so wait for `#dlg[open] …` when a dialog is reused.
- **README pictures:** `docs/screenshots/` comes from the same UI test. After a visible UI change, regenerate with `scripts/readme-screenshots.sh <with-sops .deb>` (needs Docker); it runs in a clean container so no local paths end up in the pictures.
- **Screenshots:** every CI run publishes a `screenshots` artifact. Look at them after changing styles or layout; they have already caught a rendering bug no assertion would.
- **Packages:** `open-sops-ui --self-check [file]` reports as JSON which sops the app would use, without opening a window. `scripts/smoke-test.sh` relies on it.
- **Tested systems:** the "Tested on" table in the README mirrors the matrices in `build.yml`. Change both together.
- **sops versions:** CI tests the pinned version on all platforms and the oldest supported one on Linux. The floor is `MIN_SOPS_VERSION` in `core/src/lib.rs` and in `build.yml` (3.11, the first release with `set --value-stdin`); change both together, and the README and the banner text in `ui/app.js`.
- Tests set `XDG_CONFIG_HOME` (and `AppData`) to a temp folder. Do the same when running the app by hand, so the real `keys.txt` and config are never touched.
- Not covered: native dialogs, the `.msi` installer, and OS versions older than the CI runners.
