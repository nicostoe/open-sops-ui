# Agent guide for Open SOPS UI

A Tauri 2 desktop app for SOPS-encrypted files (env, YAML, JSON, INI and plain text). See [README.md](README.md) for what it does and how to build it.

## Layout

- `core/`: the `open-sops-ui-core` crate. All backend logic: keys, recognising sops files, env parsing, `.sops.yaml` rules, calls to `sops`. No GUI dependencies, so it compiles and tests without WebKit.
  - `src/lib.rs`: `App::handle(action, body)` is the single entry point the UI calls.
  - `tests/flow.rs`: end-to-end test against a real `sops`.
- `src-tauri/`: the desktop shell. `src/main.rs` wires one `api` command to `App::handle`. `tauri.conf.json` holds window, CSP and bundle settings; `capabilities/default.json` the permissions.
- `ui/`: the frontend. `index.html`, `style.css`, `app.js`, and `highlight.js` for the text editor's syntax colours (YAML, JSON, INI; TOML and .conf by extension). Vanilla JS, no bundler, no dependencies. The colours are a copy of the text in a layer behind a transparent textarea, so `highlight.html` must return exactly the input's characters; the UI test checks that they line up.
- `ui/selftest.js` and `src-tauri/src/ui_test.rs`: the scripted UI self-test. With `OPEN_SOPS_UI_SELFTEST=<folder>` set, the app clicks through itself, takes screenshots and exits. Inactive otherwise.
- `ui-tests/selftest.mjs`: runs that test against a binary with throwaway folders and checks the files on disk. Plain Node, no dependencies.
- `.github/workflows/build.yml`: on pull requests, pushes to `main`, weekly, and called by the release workflow. Jobs: `build` (tests, packages, smoke test and UI test per platform), `distros` (packages installed and UI-tested in distribution containers), `os-versions` (UI test on the older macOS runners), `screenshots` (one artifact with every platform's pictures), `required` ("All checks passed", the single check the ruleset for `main` requires).
- `.github/workflows/release.yml`: started from the Actions tab (`beta` with a `patch`/`minor`/`major` bump, or `stable`), or by a pushed `v*` tag. Calls `build.yml`, then publishes a GitHub release. A tag with a `-suffix` becomes a pre-release. From the Actions tab, `scripts/next-release.sh` picks the tag and commit from the existing tags alone: a beta is the next `-beta.N` of `main` (the bump picks the version after the latest stable release; betas already further along carry on), stable promotes the latest beta's commit and refuses without one. The build gets that commit and tag (`build.yml` inputs `ref` and `tag`; `version.sh` reads `RELEASE_TAG`), and the tag is created only after the build passed, so a failed build leaves no tag. `scripts/test-next-release.sh` checks the numbering against throwaway tags.
- `.github/workflows/audit.yml`: weekly `cargo audit` and `npm audit`.
- `scripts/fetch-sops.sh`: downloads the pinned sops into `vendor/sops/` (git-ignored) and verifies it with cosign and SHA-256. The `SOPS_VERSION` line carries a Renovate comment; keep its format, `renovate.json` matches it with a regex.
- `scripts/smoke-test.sh`: installs the built package and runs `open-sops-ui --self-check` (CI only; it installs the app).
- `scripts/ui-test.sh`, `scripts/distro-test.sh`: run the UI test, on a runner or inside a distribution container.
- `scripts/version.sh`: the build's version from the tag (or `<next-release.sh dev>-dev.<run>` in other CI runs). Sets `OPEN_SOPS_UI_VERSION`, which is compiled into the app, and writes `src-tauri/tauri.version.conf.json` (git-ignored) with the plain x.y.z, the MSI version and the rpm release for `tauri build --config`.
- `cliff.toml`, `scripts/release-notes.sh`: release notes from commit messages, opened by the annotated tag's message if it says more than the tag name. The release workflow attaches them to each release; every CI run on Linux shows the preview for the next one on its summary page, and `scripts/test-release-notes.sh` checks them against throwaway tags.
- `scripts/collect-bundles.sh`, `scripts/ci-annotate.sh`: CI helpers for package names and public failure annotations. `collect-bundles.sh` puts the full version from `OPEN_SOPS_UI_VERSION` into the file names (`Open-SOPS-UI_1.2.0-beta.4_amd64.AppImage`), because Tauri only knows the plain x.y.z; `scripts/test-collect-bundles.sh` checks the names.
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

- **UI to backend:** `app.js` calls `window.__TAURI__.core.invoke("api", { action, body })`. Adding a feature means adding a match arm in `App::handle` and calling it from `app.js`; no new Tauri command or permission is needed. The one exception is `state` from the page: `main.rs` runs its folder scan (`App::scan_job`, `ScanJob::run`, `App::finish_scan`) without holding the app's lock, so opening and saving don't wait for a slow scan.
- **Which files are listed:** only sops-encrypted files, recognised by content (`detect`), never by name; plaintext files are neither listed nor opened. `ScanJob::run` walks the folder to the configured depth (`scan_depth`, default 3), skips hidden folders and the configured folder names (`skip_dirs`, default `DEFAULT_SKIP_DIRS`), looks at no more than `MAX_SCANNED_ENTRIES` entries and lists at most 200 files. It reads only the last 64 KB of a candidate first (`tail_looks_encrypted`), and keeps what it learned per file in `ScanCache` until the file's size or modification time changes, so a refresh reads only new and changed files.
- **Formats:** `Format` is how sops stores a file (`Store`: dotenv, YAML, JSON, INI) and whether its plaintext is one block of text (`whole`, sops' "binary" type, stored as a document with a single `data` value). Every sops call passes explicit `--input-type`/`--output-type` from it (`decrypt_args`, `encrypt_args`, `store_args`); a name like `secrets.enc` says nothing about the format. Env files open as a table of variables (`kind: "env"`), everything else as text (`kind: "text"`). New files get their format from the name (`format_for_name`).
- **sops lookup:** a `sops` / `sops.exe` in the executable's folder or the bundle's resource folder wins over `PATH` (`bundled_dirs`). Never add the folder an AppImage file sits in, or any other folder users download into: a stray `sops` there would run with their keys. `scripts/smoke-test.sh` places a decoy next to the AppImage to guard this. `find_program` also checks Homebrew and user-local folders, because apps started from a launcher get a short `PATH`.
- **Keys:** all loaded age identities are joined and passed to sops per call through `SOPS_AGE_KEY`. Public keys come from the `age` crate.
- **Saving an env file:** compares the UI's variables with the decrypted file and applies each difference with `sops set --value-stdin` or `sops unset`. Values travel through stdin, so no decrypted content reaches the disk (`sops edit` writes a temp file, which is why it isn't used), and untouched variables keep their ciphertext. If a step fails, the original bytes are written back. Names that can't be addressed as `['NAME']` fall back to re-encrypting the whole file (`reencrypt`).
- **Saving a text file:** a `whole` file is one value, replaced in place with `sops set --value-stdin … '["data"]'`, which keeps every key and option. A YAML, JSON or INI file is encrypted again through stdin (`reencrypt`) for the age recipients in its metadata, passing on the options sops records there (`KEPT_OPTIONS`: encrypted/unencrypted regex and suffix). What flags can't reproduce (other key types such as PGP or KMS, `mac_only_encrypted`, comment regexes, several key groups) makes `reencrypt_problem` refuse, rather than silently dropping it; the UI then shows the file read-only. Windows line endings in the decrypted text are kept.
- **Refreshing:** the UI reads the state again on the refresh button and whenever the window regains focus (at most every two seconds), so files added by a `git pull` appear. No file watcher: it would cost resources and behave differently per OS. Each listed file carries its `rev`; when the open file's differs, it reloads silently without unsaved edits, or keeps them and shows a note.
- **Changed on disk:** `open` returns a fingerprint (`rev`) of the file; `save` sends it back and refuses with `{conflict: true}` when the file changed meanwhile, until the user confirms (`overwrite`).
- **Encodings:** `read_text` decodes UTF-8 (with or without BOM), UTF-16 and Windows-1252 and rejects files that aren't text or are over 2 MB. Use it for every encrypted or key file; `read_to_string` is only for files the app or git wrote. Decrypted text that isn't valid UTF-8 (a binary secret) is refused with a message instead of being shown mangled (`plaintext`).
- **Clipboard:** the page has no clipboard permission. Copying goes through the `copy` command in `main.rs`, which removes a secret again after the configured time and when the app exits. Clipboard managers restore emptied clipboards, so after clearing it checks and, if the secret came back, replaces it with a short note.
- **New key pairs for elsewhere:** `machineKeyCreate` keeps the private key in `App::machine_key` (memory only) and returns just the public key. The page copies it with `copy` and `machineKey: true`, so `main.rs` takes it from the backend and it never passes through the page. `machineKeyFinish` saves the public key as a recipient and drops the private key; `machineKeyDiscard` drops both.
- **New files:** `checkNewFile` validates folder and name without writing and reports the format; `create` encrypts the content (variables or text) through stdin (`encrypt_text`) and writes the result with `create_new`, so an existing file is never replaced and no plaintext reaches the disk.
- **Changing recipients:** decrypts and pipes the plaintext to `sops encrypt` on stdin (`reencrypt`), with the same limits as saving a text file.
- **Working directory:** sops finds `.sops.yaml` upward from its working directory and fails if one exists without a matching rule. Calls with explicit recipients run from `neutral_dir()`; only rule-based encryption runs from the file's folder.
- **Git safety:** every git call goes through `git_read`, which switches off the programs a repository's own config can make git run (`core.fsmonitor`, hooks). Opening a folder must never execute anything from it; `untrusted_repository_config_is_not_executed` guards this. Never call `run(git, …)` directly.
- **Git:** optional everywhere. Repository and branch come from the file system (`repo_root`, `repo_branch` read `.git`), so grouping works without the `git` binary. Status needs `git` and costs two calls per repository (`git_states`), never one per file. `changes` decrypts `git show HEAD:<file>` and compares by variable for env files and by line for everything else (`text_changes`). The app only reads from Git; it never commits, pushes, or writes decrypted values to disk.
- **Versions:** come from the Git tags alone; no file has to be raised for a release. The app's version is `OPEN_SOPS_UI_VERSION` from the build (`build_version` in `main.rs`), falling back to `tauri.conf.json` locally, which is all that version is for. Package metadata gets the plain `x.y.z` through `tauri.version.conf.json`; never let a pre-release reach a package's version, because `.rpm` rejects it and `.deb` misorders it.
- **Update notice:** the page fetches `api.github.com/repos/nicostoe/open-sops-ui/releases` (allowed in the CSP's `connect-src`) on start and every 24 hours, unless turned off. `pick_update` in the core decides what to announce: newer than this build, and a stable build never hears about a pre-release. `open_release_page` opens only this project's release URL, built from a validated tag. Nothing is downloaded. A check gives up after 10 seconds (`UPDATE_TIMEOUT_MS`), only one runs at a time, and a failure is silent unless the user pressed "Check now"; a notice already shown stays.

## Commit messages

Release notes are generated from commit subjects (`cliff.toml`). Only changes someone using the app notices get a prefix, following [Conventional Commits](https://www.conventionalcommits.org), whose standard defines just these two types:

| Subject | Section in the release notes |
| --- | --- |
| `feat: …` | Features |
| `fix: …` | Bug fixes (security fixes too) |
| `[renovate] …` | Dependencies (Renovate writes these) |
| anything else | left out |

- Internal work (CI, tests, docs, refactoring, tooling) has no prefix and stays out of the notes. Don't invent other types such as `ci:` or `chore:`.
- Write the description for someone using the app ("fix: keep the scroll position after saving"), not about the code. Mark a breaking change with `!` (`feat!: …`). A scope is allowed (`fix(security): …`) and still counts as a fix.
- A user-visible change without `feat:` or `fix:` silently disappears from the notes, so check the prefix before committing.
- Preview the notes with `scripts/release-notes.sh` (needs `cargo install git-cliff --locked`).

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

- **Git:** `git_flow` in `tests/flow.rs` covers status and comparison against a real repository; `works_without_git_installed` in `lib.rs` covers the app with no `git` binary. The UI test's project is a repository, and its test-only `commit` action (in `ui_test.rs`) creates the commit to compare against.
- **Backend:** `cargo test -p open-sops-ui-core`. `tests/flow.rs` is one end-to-end flow against a real sops; extend it when you add or change an action. It skips itself when no `sops` is on `PATH`; put the one from `scripts/fetch-sops.sh` there to run it. `text_formats_flow` encrypts YAML, JSON, INI and text files with sops directly, as a project would, and covers recognising, saving, options and the scan settings. The test sets `EDITOR` to a program that doesn't exist, so a save that tried to use an editor would fail.
- **UI:** `node ui-tests/selftest.mjs target/release/open-sops-ui`. The steps live in `ui/selftest.js`; extend them when you change `ui/` or add a flow, add a `shot(...)` for every new screen, and update `EXPECTED` and `EXPECTED_SHOTS` in `ui-tests/selftest.mjs`. Native file dialogs can't be driven; the test passes the folder as an argument instead. The project's files are encrypted with sops before the app starts, for a key `selftest.mjs` generates (`ageKeyPair`, no `age-keygen` needed); the page gets that key and further encrypted files through the test-only `fixture` action, and `fixture.json` is deleted when the app exits. Wait for a specific element, never for text that is also in a static hint. A closed dialog keeps its old content, so wait for `#dlg[open] …` when a dialog is reused.
- **README pictures:** `docs/screenshots/` comes from the same UI test. After a visible UI change, regenerate with `scripts/readme-screenshots.sh <with-sops .deb>` (needs Docker); it runs in a clean container so no local paths end up in the pictures.
- **Screenshot privacy:** in test mode the window is titled `ui_test::WINDOW_TITLE`, and on Linux screenshots find the window by that title. Never go back to the normal title: a run on a developer's desktop would then capture their own open copy of the app. On macOS and Windows the test captures the whole screen, so run it only on CI runners or other machines without personal windows.
- **Screenshots:** every CI run publishes a `screenshots` artifact. Look at them after changing styles or layout; they have already caught a rendering bug no assertion would.
- **Packages:** `open-sops-ui --self-check [file]` reports as JSON which sops the app would use and its version, without opening a window. `scripts/smoke-test.sh` relies on it and checks the version matches `OPEN_SOPS_UI_VERSION`.
- **Updates:** `picks_the_update_to_announce` in `lib.rs` covers the rules. The UI test gets a fixed release list (`releases.json`, written by `ui-tests/selftest.mjs`) instead of GitHub's, and in test mode `open_release_page` records the tag instead of opening a browser. The fixture can also be `{"simulate": "error" | "hang", "timeoutMs": n}`, which the page turns into a failed or never-answering request; the UI test uses both. CI builds are pre-releases (`-dev.N`), local ones stable, so the test checks whichever rule applies.
- **Required check:** `main` only accepts pull requests whose "All checks passed" job is green. It waits for `build` and `distros`; add every new job that must pass before merging to its `needs`. Leave out jobs whose runners are often unavailable (the macOS 15 checks), or every runner shortage blocks merging. `.github/CODEOWNERS` asks the owner to review every pull request.
- **Tested systems:** the "Tested on" table in the README mirrors the matrices in `build.yml`. Change both together.
- **sops versions:** CI tests the pinned version on all platforms and the oldest supported one on Linux. The floor is `MIN_SOPS_VERSION` in `core/src/lib.rs` and in `build.yml` (3.11, the first release with `set --value-stdin`); change both together, and the README and the banner text in `ui/app.js`.
- Tests set `XDG_CONFIG_HOME` (and `AppData`) to a temp folder. Do the same when running the app by hand, so the real `keys.txt` and config are never touched.
- Not covered: native dialogs, the `.msi` installer, and OS versions older than the CI runners.
