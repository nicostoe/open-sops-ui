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

1. Install `sops` 3.9 or newer from [github.com/getsops/sops/releases](https://github.com/getsops/sops/releases), or put the `sops` binary in the same folder as the app. A binary next to the app is used first, then the one on your `PATH`.
2. Build the app for your system (see below) and install the package from `target/release/bundle/`:

   | OS | Package |
   | --- | --- |
   | Linux | `.deb`, `.rpm` or `.AppImage` |
   | macOS | `.dmg` |
   | Windows | `.msi` or `-setup.exe` |

3. Start "Sops Env" from your app menu, or from a terminal with a project folder:

   ```bash
   sops-env path/to/project
   ```

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

`.github/workflows/build.yml` does this on GitHub: it runs on pull requests, pushes to `main`, `v*` tags and on demand, and attaches the packages for Linux, Windows and macOS to the workflow run as artifacts.

## Test

```bash
cargo test -p sops-env-core
```

The end-to-end test runs against your installed `sops` with throwaway keys in a temp folder. It's skipped when `sops` isn't installed.

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
