# Sops Env

A desktop app for viewing, editing and encrypting env files with [SOPS](https://github.com/getsops/sops) and age keys. Built with [Tauri](https://tauri.app) for Linux, Windows and macOS.

Decrypted values are held in memory only. The app makes no network requests.

## What it does

- Lists env files in a folder and shows which are encrypted, which of your keys opens each, and which are still plaintext.
- Edits values in a table and saves through `sops edit`, so unchanged values stay byte-identical in Git.
- Loads age keys from the default location, from any number of custom key files, or pasted for the session only. A pasted or newly created key can be saved to the default location.
- Manages who can decrypt a file, with a saved list of named public keys.
- Warns when a file's recipients differ from the matching `.sops.yaml` rule.
- Light and dark mode, following the system by default.

## Install

1. Download the package for your system from the [releases page](https://github.com/nicostoe/open-sops-ui/releases). Each one comes in two variants:
   - `…-with-sops`: includes the official `sops` binary, nothing else to install.
   - without the suffix: uses the `sops` 3.10 or newer on your `PATH`. Get it from [github.com/getsops/sops/releases](https://github.com/getsops/sops/releases).
2. Install it:

   | OS | Package |
   | --- | --- |
   | Linux | `.deb`, `.rpm` or `.AppImage` |
   | macOS | `.dmg` |
   | Windows | `.msi` or `-setup.exe` |

3. Start "Sops Env" from your app menu, or from a terminal with a project folder:

   ```bash
   sops-env path/to/project
   ```

A `sops` binary shipped with the app, or placed next to it, is used first; otherwise the one on your `PATH`.

`git` is optional; it's used to warn about plaintext env files that Git isn't ignoring. `age` and `age-keygen` aren't needed.

The packages aren't signed. macOS and Windows may warn on first start.

## Build

Needs [Rust](https://rustup.rs), Node.js 20 or newer, and the [Tauri system dependencies](https://tauri.app/start/prerequisites/). On Debian, Ubuntu or Mint:

```bash
sudo apt install -y libwebkit2gtk-4.1-dev libsoup-3.0-dev librsvg2-dev build-essential libssl-dev
```

Then:

```bash
npm install
```

```bash
npm run dev
```

```bash
npm run build
```

Tauri can't cross-compile: each system's package has to be built on that system (or in CI on a runner for it).

`.github/workflows/build.yml` does this on GitHub: it runs on pull requests, pushes to `main` and on demand, runs the tests described below, and attaches the packages for Linux, Windows and macOS to the workflow run as artifacts.

## Release

Pushing a version tag publishes a GitHub release with all packages, both variants, and a `SHA256SUMS.txt`:

```bash
git tag v0.1.0-beta.1 && git push origin v0.1.0-beta.1
```

A tag with a suffix after the version (`-beta.1`, `-rc.1`) is published as a pre-release; a plain `v0.1.0` as a normal release.

The release runs the full build workflow first, so nothing is published unless every test passes. The tag has to match `version` in `src-tauri/tauri.conf.json`, because the package file names come from there; for a new version, change that one line, commit, then tag.

The bundled `sops` version is pinned in `scripts/fetch-sops.sh`. The script verifies the release's checksums file with cosign and then the binary's SHA-256. [Renovate](https://docs.renovatebot.com) opens a pull request when a new `sops` release appears (`renovate.json`); the Renovate GitHub App has to be installed on the repository for that. `sops` is licensed under the MPL 2.0, and its licence text ships next to the binary.

## Test

Backend, against your installed `sops` with throwaway keys in a temp folder (skipped when `sops` isn't installed):

```bash
cargo test -p sops-env-core
```

UI: the app clicks through itself in its real window, takes a screenshot at each stage, and the result is checked on disk with `sops`. It needs a display and, on Linux, ImageMagick for the screenshots:

```bash
node ui-tests/selftest.mjs target/release/sops-env
```

Screenshots land in `ui-test-output/`. The test uses its own temp folders; your keys and config aren't touched.

CI runs this on every pull request and once a week:

| Check | Where |
| --- | --- |
| Backend against the pinned `sops` | Linux, Windows, macOS |
| Backend against the oldest supported `sops` (3.10.0) | Linux |
| Install the package, start the app, find the bundled `sops` | Ubuntu 22.04 (`.deb`, AppImage), Windows Server 2025, macOS 26 |
| UI test with screenshots in the installed app | the three above, plus macOS 15 (Apple silicon and Intel) and Windows Server 2022 |
| Install the package and run the UI test in a container | Debian 12 and 13, Ubuntu 22.04, 24.04 and rolling, Fedora (`.rpm`) |

Each run offers one `screenshots` artifact with the pictures from every platform, and its summary lists which platforms passed. A failed check repeats its error as an annotation on the run, readable without signing in.

Not covered by any test: native file dialogs, the clipboard, the `.msi` installer, and macOS or Windows versions older than the ones above.

## Keeping it up to date

- **Renovate** (`renovate.json`) opens pull requests on Monday mornings. Minor and patch updates come as one grouped pull request and merge themselves once CI is green; so does a new `sops` release, in its own pull request. Major updates wait for a manual merge.
- **Audit** (`.github/workflows/audit.yml`) checks weekly for known vulnerabilities in the Rust and npm dependencies.
- For auto-merge to be safe, protect `main` on GitHub and make the four `Build` jobs required checks.

## Where things are stored

| OS | Default key file |
| --- | --- |
| Linux | `$XDG_CONFIG_HOME/sops/age/keys.txt` or `~/.config/sops/age/keys.txt` |
| macOS | `$XDG_CONFIG_HOME/sops/age/keys.txt` or `~/Library/Application Support/sops/age/keys.txt` |
| Windows | `%AppData%\sops\age\keys.txt` |

Custom key paths, saved recipients and the last folder are stored in `sops-env/config.json` next to the `sops` folder above. Secret keys are never written there.

## Limits

- Only the dotenv format and age keys are supported (no PGP or cloud KMS, no YAML or JSON files).
- Values are single-line; comments and blank lines are kept but not editable.
