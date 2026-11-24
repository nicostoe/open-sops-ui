# Open SOPS UI

A desktop app for viewing and editing [SOPS](https://github.com/getsops/sops)-encrypted files with age keys. For Linux, Windows and macOS, built with [Tauri](https://tauri.app).

Decrypted values stay in memory. The only network request is an optional update check against GitHub.

<p align="center">
  <img src="docs/screenshots/editor-light.png" width="49%" alt="Editing an encrypted env file: one value changed, one variable added">
  <img src="docs/screenshots/changes-dark.png" width="49%" alt="Dark mode: what changed in an encrypted file since the last commit">
</p>
<p align="center">
  <img src="docs/screenshots/text-editor-light.png" width="49%" alt="An encrypted YAML file opened as text, with syntax colours">
</p>

## What it does

- Finds every SOPS-encrypted file in a folder: env, YAML, JSON, INI and plain text.
- Edits env files as a table of variables, everything else as text with syntax colours.
- Saves env changes value by value, so unchanged values stay identical in Git.
- Shows what changed since the last commit, decrypted.
- Groups files by Git repository and branch. Git is optional.
- Manages your age keys and who can decrypt each file.
- Creates key pairs for servers and pipelines; the private key is copied once, never stored.
- Clears copied secrets from the clipboard after a set time.
- Warns when a file's recipients differ from its `.sops.yaml` rule.
- Notices changes from a `git pull` and asks before overwriting.
- Creates new files already encrypted.
- Light and dark mode.

## How it works

The app is a front end for the `sops` command line tool. It finds files by their sops metadata, not their name, and calls `sops` with your age keys to read and write them. Decrypted content only travels to `sops` through stdin, so nothing decrypted is written to disk, not even a temp file. The app reads from Git but never commits or pushes.

Keys come from the default `keys.txt`, from other key files, or are pasted for the session.

## Install

1. Download a package from the [releases page](https://github.com/nicostoe/open-sops-ui/releases): `.deb`, `.rpm` or `.AppImage` for Linux, `.dmg` for macOS, `.msi` or `-setup.exe` for Windows.
   - `…-with-sops` includes `sops`.
   - The others use `sops` 3.11 or newer from your `PATH` ([download](https://github.com/getsops/sops/releases)).
2. Start "Open SOPS UI" from your app menu, or with a folder:

   ```bash
   open-sops-ui path/to/project
   ```

You don't need `age` or `git`. The packages aren't signed, so macOS and Windows may warn on first start.

## Tested on

Every pull request installs the app and clicks through it on:

| System | Versions |
| --- | --- |
| Windows | Server 2025 (shares its web view with Windows 11) |
| macOS | 26 and 15, Apple silicon and Intel |
| Debian | 12 and 13 |
| Ubuntu | 22.04, 24.04 and rolling |
| Fedora | current release |

## Limits

- Only age keys. YAML, JSON and INI files that also use PGP or a cloud KMS open read-only.
- Env values are single-line.

## Where things are stored

| OS | Default key file |
| --- | --- |
| Linux | `~/.config/sops/age/keys.txt` (or `$XDG_CONFIG_HOME/sops/age/keys.txt`) |
| macOS | `~/Library/Application Support/sops/age/keys.txt` (or `$XDG_CONFIG_HOME/…`) |
| Windows | `%AppData%\sops\age\keys.txt` |

Settings, saved recipients and recent folders go to `open-sops-ui/config.json` next to the `sops` folder. Secret keys are never written there.

## Development

Needs [Rust](https://rustup.rs), Node.js 20+ and the [Tauri prerequisites](https://tauri.app/start/prerequisites/).

```bash
npm install
```

```bash
npm run dev
```

```bash
cargo test -p open-sops-ui-core
```

CI builds and tests every pull request on all platforms, including a UI test that clicks through the real app. Releases are started from the Actions tab: a beta first, then the stable release. Renovate keeps dependencies current. [AGENTS.md](AGENTS.md) describes the architecture, tests and release process in detail.

## License

[MIT](LICENSE). The `-with-sops` packages also contain `sops`, licensed under the [MPL 2.0](https://github.com/getsops/sops/blob/main/LICENSE).
