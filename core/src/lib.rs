//! Backend logic for open-sops-ui. No GUI code lives here, so it can be tested
//! on its own against a real `sops` binary.
//!
//! The UI talks to [`App::handle`] with an action name and a JSON body.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::str::FromStr;

use age::secrecy::ExposeSecret;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

type Res<T> = Result<T, String>;

// ---------------------------------------------------------------------------
// Paths and config
// ---------------------------------------------------------------------------

fn home_dir() -> PathBuf {
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    PathBuf::from(std::env::var(var).unwrap_or_else(|_| ".".into()))
}

/// The folder sops itself uses for its config, per OS.
fn config_base() -> PathBuf {
    if cfg!(windows) {
        return std::env::var("AppData")
            .map(PathBuf::from)
            .unwrap_or_else(|_| home_dir().join("AppData").join("Roaming"));
    }
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        if !xdg.is_empty() {
            return PathBuf::from(xdg);
        }
    }
    if cfg!(target_os = "macos") {
        home_dir().join("Library").join("Application Support")
    } else {
        home_dir().join(".config")
    }
}

pub fn default_key_path() -> PathBuf {
    config_base().join("sops").join("age").join("keys.txt")
}

fn config_path() -> PathBuf {
    config_base().join("open-sops-ui").join("config.json")
}

/// sops looks for .sops.yaml upward from its working directory and fails when
/// one exists but no rule matches, even with explicit recipients. Calls that
/// must not depend on a project's config run from this folder instead.
fn neutral_dir() -> Option<PathBuf> {
    let dir = config_base().join("open-sops-ui");
    std::fs::create_dir_all(&dir).ok().map(|_| dir)
}

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase", default)]
struct Config {
    key_paths: Vec<KeyPath>,
    book: Vec<BookEntry>,
    last_folder: Option<String>,
    /// Recently opened folders, newest first.
    recent: Vec<String>,
    /// Seconds after which a copied value is removed from the clipboard; 0 keeps it.
    clipboard_clear_seconds: Option<u32>,
    /// Whether to look for a newer release on start and once a day; on unless turned off.
    check_updates: Option<bool>,
    /// How many folder levels below the open folder the file list searches.
    scan_depth: Option<u32>,
    /// Folder names the scan doesn't enter; the defaults when unset.
    skip_dirs: Option<Vec<String>>,
}

const DEFAULT_CLIPBOARD_CLEAR_SECONDS: u32 = 60;

#[derive(Serialize, Deserialize, Clone)]
struct KeyPath {
    path: String,
    label: String,
}

#[derive(Serialize, Deserialize, Clone)]
struct BookEntry {
    name: String,
    key: String,
}

fn load_config() -> Config {
    // The app was called "Sops Env" in its first pre-releases; fall back to
    // that config so custom key paths and saved recipients carry over.
    let legacy = config_base().join("sops-env").join("config.json");
    std::fs::read_to_string(config_path())
        .or_else(|_| std::fs::read_to_string(legacy))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Reading text files
// ---------------------------------------------------------------------------
//
// Encrypted files are always UTF-8, because sops writes them. Plaintext env
// files and key files come from anywhere: Windows PowerShell writes UTF-16,
// old editors Windows-1252, some tools a UTF-8 byte order mark.

const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;

/// Windows-1252 differs from Latin-1 only in this range.
const CP1252_HIGH: [char; 32] = [
    '\u{20AC}', '\u{81}', '\u{201A}', '\u{192}', '\u{201E}', '\u{2026}', '\u{2020}', '\u{2021}',
    '\u{2C6}', '\u{2030}', '\u{160}', '\u{2039}', '\u{152}', '\u{8D}', '\u{17D}', '\u{8F}',
    '\u{90}', '\u{2018}', '\u{2019}', '\u{201C}', '\u{201D}', '\u{2022}', '\u{2013}', '\u{2014}',
    '\u{2DC}', '\u{2122}', '\u{161}', '\u{203A}', '\u{153}', '\u{9D}', '\u{17E}', '\u{178}',
];

fn decode_utf16(bytes: &[u8], little_endian: bool) -> Option<String> {
    let units = bytes.chunks_exact(2).map(|pair| {
        if little_endian {
            u16::from_le_bytes([pair[0], pair[1]])
        } else {
            u16::from_be_bytes([pair[0], pair[1]])
        }
    });
    char::decode_utf16(units)
        .collect::<Result<String, _>>()
        .ok()
}

/// Decodes a text file's bytes and names the encoding found. `None` means
/// the bytes aren't text.
fn decode_text(bytes: &[u8]) -> Option<(String, &'static str)> {
    let zeros_at = |offset: usize| {
        bytes
            .iter()
            .skip(offset)
            .step_by(2)
            .filter(|b| **b == 0)
            .count()
    };
    let decoded = if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        (String::from_utf8(rest.to_vec()).ok()?, "UTF-8 with BOM")
    } else if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        (decode_utf16(rest, true)?, "UTF-16")
    } else if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        (decode_utf16(rest, false)?, "UTF-16")
    } else if let Ok(text) = std::str::from_utf8(bytes) {
        (text.to_string(), "UTF-8")
    } else if bytes.len().is_multiple_of(2) && zeros_at(1) * 4 > bytes.len() {
        // No byte order mark, but every other byte is zero: UTF-16 of mostly ASCII text.
        (decode_utf16(bytes, true)?, "UTF-16")
    } else if bytes.len().is_multiple_of(2) && zeros_at(0) * 4 > bytes.len() {
        (decode_utf16(bytes, false)?, "UTF-16")
    } else {
        let text = bytes
            .iter()
            .map(|b| {
                if (0x80..0xA0).contains(b) {
                    CP1252_HIGH[(*b - 0x80) as usize]
                } else {
                    *b as char
                }
            })
            .collect();
        (text, "Windows-1252")
    };
    (!decoded.0.contains('\0')).then_some(decoded)
}

struct TextFile {
    text: String,
    encoding: &'static str,
    /// The bytes as they are on disk, to put back if a save fails halfway.
    bytes: Vec<u8>,
    /// Fingerprint of the bytes, to notice when someone else changes the file.
    rev: String,
}

enum ReadProblem {
    Missing,
    Unreadable(String),
}

impl ReadProblem {
    fn message(&self) -> String {
        match self {
            ReadProblem::Missing => "File or folder not found.".into(),
            ReadProblem::Unreadable(why) => format!("{why}."),
        }
    }
}

fn fingerprint(bytes: &[u8]) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn read_text(path: &Path) -> Result<TextFile, ReadProblem> {
    let problem = |e: std::io::Error| match e.kind() {
        std::io::ErrorKind::NotFound => ReadProblem::Missing,
        _ => ReadProblem::Unreadable(io_msg(e).trim_end_matches('.').to_string()),
    };
    if std::fs::metadata(path).map_err(problem)?.len() > MAX_FILE_BYTES {
        return Err(ReadProblem::Unreadable(
            "Larger than 2 MB, too big for an env file".into(),
        ));
    }
    let bytes = std::fs::read(path).map_err(problem)?;
    let (text, encoding) =
        decode_text(&bytes).ok_or_else(|| ReadProblem::Unreadable("Not a text file".into()))?;
    Ok(TextFile {
        text,
        encoding,
        rev: fingerprint(&bytes),
        bytes,
    })
}

// ---------------------------------------------------------------------------
// Keys
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct Identity {
    secret: String,
    public: String,
}

#[derive(Clone)]
struct KeySource {
    id: String,
    kind: &'static str, // "default" | "path" | "session"
    label: String,
    path: Option<String>,
    identities: Vec<Identity>,
    error: Option<String>,
}

fn parse_identities(text: &str) -> Res<Vec<Identity>> {
    let mut out = Vec::new();
    for line in text.lines().map(str::trim) {
        if !line.starts_with("AGE-SECRET-KEY-") {
            continue;
        }
        let id = age::x25519::Identity::from_str(line)
            .map_err(|_| "That doesn't look like a valid age secret key.".to_string())?;
        out.push(Identity {
            secret: line.to_string(),
            public: id.to_public().to_string(),
        });
    }
    Ok(out)
}

fn read_key_file(path: &Path) -> (Vec<Identity>, Option<String>) {
    match read_text(path) {
        Err(ReadProblem::Missing) => (vec![], Some("No file found here".into())),
        Err(ReadProblem::Unreadable(why)) => (vec![], Some(why)),
        Ok(file) => match parse_identities(&file.text) {
            Ok(ids) if ids.is_empty() => (ids, Some("No age key found in this file".into())),
            Ok(ids) => (ids, None),
            Err(e) => (vec![], Some(e)),
        },
    }
}

fn valid_public_key(key: &str) -> bool {
    age::x25519::Recipient::from_str(key).is_ok()
}

fn utc_timestamp() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Civil-from-days (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// Appends identities to the default keys.txt. Existing keys are never touched.
fn append_to_default(identities: &[Identity]) -> Res<()> {
    let path = default_key_path();
    // Appending UTF-8 to a file in another encoding would corrupt it.
    let existing = match read_text(&path) {
        Ok(file) if file.encoding == "UTF-8" => file.text,
        Ok(file) => {
            return Err(format!(
                "Your keys.txt is stored as {}. Save it as UTF-8 first; the app won't rewrite it.",
                file.encoding
            ))
        }
        Err(ReadProblem::Missing) => String::new(),
        Err(problem) => return Err(problem.message()),
    };
    let fresh: Vec<&Identity> = identities
        .iter()
        .filter(|i| !existing.contains(&i.secret))
        .collect();
    if fresh.is_empty() {
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(io_msg)?;
    }
    // Only what is new gets written, at the end. The keys already in the
    // file are never rewritten, so a crash or a full disk can't lose them.
    let mut addition = String::new();
    if !existing.is_empty() && !existing.ends_with('\n') {
        addition.push('\n');
    }
    for i in fresh {
        addition.push_str(&format!(
            "# created: {}\n# public key: {}\n{}\n",
            utc_timestamp(),
            i.public,
            i.secret
        ));
    }
    append_private(&path, &addition)
}

/// Appends to a file only its owner may read, creating it if needed.
fn append_private(path: &Path, text: &str) -> Res<()> {
    let mut opts = std::fs::OpenOptions::new();
    opts.append(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut file = opts.open(path).map_err(io_msg)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).map_err(io_msg)?;
    }
    file.write_all(text.as_bytes()).map_err(io_msg)?;
    file.sync_all().map_err(io_msg)
}

fn io_msg(e: std::io::Error) -> String {
    match e.kind() {
        std::io::ErrorKind::NotFound => "File or folder not found.".into(),
        std::io::ErrorKind::PermissionDenied => "Permission denied.".into(),
        _ => e.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Running external programs
// ---------------------------------------------------------------------------

struct Output {
    code: i32,
    stdout: String,
    /// False when stdout wasn't valid UTF-8 and `stdout` had to replace bytes.
    utf8: bool,
    stderr: String,
}

fn run(
    program: &Path,
    args: &[&str],
    env: &[(&str, String)],
    cwd: Option<&Path>,
    stdin: Option<&str>,
) -> std::io::Result<Output> {
    let mut cmd = Command::new(program);
    cmd.args(args).stdout(Stdio::piped()).stderr(Stdio::piped());
    cmd.stdin(if stdin.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    });
    for (k, v) in env {
        cmd.env(k, v);
    }
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    #[cfg(windows)]
    {
        // A GUI app must not flash a console window for every child process.
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let mut child = cmd.spawn()?;
    if let (Some(text), Some(mut pipe)) = (stdin, child.stdin.take()) {
        pipe.write_all(text.as_bytes())?;
    }
    let out = child.wait_with_output()?;
    Ok(Output {
        code: out.status.code().unwrap_or(-1),
        utf8: std::str::from_utf8(&out.stdout).is_ok(),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    })
}

/// Looks a program up on PATH, then in the usual install folders. Apps started
/// from a desktop launcher (macOS Finder above all) get a short PATH that
/// leaves out Homebrew and user-local folders.
fn find_program(name: &str) -> Option<PathBuf> {
    let exts: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".EXE;.CMD;.BAT".into())
            .split(';')
            .filter(|e| !e.is_empty())
            .map(str::to_string)
            .collect()
    } else {
        vec![String::new()]
    };
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    if cfg!(unix) {
        let home = home_dir();
        dirs.extend([
            PathBuf::from("/opt/homebrew/bin"),
            PathBuf::from("/usr/local/bin"),
            PathBuf::from("/usr/bin"),
            PathBuf::from("/home/linuxbrew/.linuxbrew/bin"),
            home.join(".local").join("bin"),
            home.join("bin"),
            home.join(".nix-profile").join("bin"),
        ]);
    }
    for dir in dirs {
        for ext in &exts {
            let cand = dir.join(format!("{name}{ext}"));
            if cand.is_file() {
                return Some(cand);
            }
        }
    }
    None
}

/// Oldest sops release the app works with; CI tests against it. Saving needs
/// `sops set --value-stdin`, which 3.11 introduced.
pub const MIN_SOPS_VERSION: (u32, u32) = (3, 11);

#[derive(Serialize, Clone, Default)]
struct SopsInfo {
    source: Option<&'static str>, // "bundled" | "system"
    path: Option<String>,
    version: Option<String>,
    /// False when the version is older than `MIN_SOPS_VERSION`.
    supported: bool,
}

fn version_supported(version: &str) -> bool {
    let mut parts = version.split('.').map(|p| p.parse::<u32>().unwrap_or(0));
    (parts.next().unwrap_or(0), parts.next().unwrap_or(0)) >= MIN_SOPS_VERSION
}

/// A sops binary shipped next to the app wins over the one on PATH.
fn detect_sops(bundled_dirs: &[PathBuf]) -> SopsInfo {
    let exe = if cfg!(windows) { "sops.exe" } else { "sops" };
    let mut candidates: Vec<(&'static str, PathBuf)> = bundled_dirs
        .iter()
        .map(|d| ("bundled", d.join(exe)))
        .filter(|(_, p)| p.is_file())
        .collect();
    if let Some(p) = find_program("sops") {
        candidates.push(("system", p));
    }
    let version_re = Regex::new(r"sops\s+([\d.]+)").unwrap();
    for (source, path) in candidates {
        let Ok(out) = run(&path, &["--version"], &[], None, None) else {
            continue;
        };
        if let (0, Some(m)) = (out.code, version_re.captures(&out.stdout)) {
            return SopsInfo {
                source: Some(source),
                path: Some(path.to_string_lossy().into_owned()),
                supported: version_supported(&m[1]),
                version: Some(m[1].to_string()),
            };
        }
    }
    SopsInfo::default()
}

/// A headless report of what the app would use, for packaging checks.
pub fn self_check(bundled_dirs: &[PathBuf], app_version: &str) -> Value {
    json!({
        "appVersion": app_version,
        "os": std::env::consts::OS,
        "sops": detect_sops(bundled_dirs),
        "defaultKeyPath": default_key_path().to_string_lossy(),
    })
}

fn sops_error(stderr: &str) -> String {
    let lower = stderr.to_lowercase();
    // The wording differs between platforms and sops versions.
    if [
        "no identity matched",
        "failed to decrypt",
        "could not decrypt data key",
        "did not find keys",
        "failed to get the data key",
        "no master key was able to decrypt",
        "failed to load age identities",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
    {
        return "None of your loaded keys can decrypt this file.".into();
    }
    stderr
        .lines()
        .map(str::trim)
        .rfind(|l| !l.is_empty())
        .unwrap_or("sops failed.")
        .to_string()
}

/// The plaintext sops printed, or why it can't be shown.
fn plaintext(out: Output) -> Res<String> {
    if out.code != 0 {
        return Err(sops_error(&out.stderr));
    }
    if !out.utf8 || out.stdout.contains('\0') {
        return Err("This file holds binary data, which the app can't show as text.".into());
    }
    Ok(out.stdout)
}

// ---------------------------------------------------------------------------
// Env file model
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "t", rename_all = "lowercase")]
pub enum Item {
    Kv { k: String, v: String },
    Raw { text: String },
}

pub fn parse_env(text: &str) -> Vec<Item> {
    let text = text.replace("\r\n", "\n");
    let mut lines: Vec<&str> = text.split('\n').collect();
    if lines.last() == Some(&"") {
        lines.pop();
    }
    lines
        .into_iter()
        .map(|line| match line.find('=') {
            Some(eq)
                if eq > 0 && !line.trim().is_empty() && !line.trim_start().starts_with('#') =>
            {
                Item::Kv {
                    k: line[..eq].trim().to_string(),
                    v: line[eq + 1..].to_string(),
                }
            }
            _ => Item::Raw {
                text: line.to_string(),
            },
        })
        .collect()
}

pub fn serialize_env(items: &[Item]) -> Res<String> {
    let name_re = Regex::new(r"^[A-Za-z_][A-Za-z0-9_.-]*$").unwrap();
    let mut seen = BTreeSet::new();
    let mut out = String::new();
    for item in items {
        match item {
            Item::Raw { text } => out.push_str(&text.replace(['\r', '\n'], " ")),
            Item::Kv { k, v } => {
                if !name_re.is_match(k) {
                    return Err(format!("\"{k}\" isn't a valid variable name."));
                }
                if k.starts_with("sops_") {
                    return Err("Names starting with \"sops_\" are reserved by SOPS.".into());
                }
                if !seen.insert(k.clone()) {
                    return Err(format!("{k} is defined twice."));
                }
                if v.contains(['\r', '\n']) {
                    return Err(format!("The value of {k} contains a line break."));
                }
                out.push_str(k);
                out.push('=');
                out.push_str(v);
            }
        }
        out.push('\n');
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// SOPS files: formats and metadata
// ---------------------------------------------------------------------------
//
// sops encrypts dotenv, YAML, JSON and INI files value by value, and anything
// else as one block of text ("binary"), stored as a JSON or YAML document
// with a single `data` value. Which one a file is shows in its content, not
// its name, so the app recognises files by their sops metadata.

/// How sops stores a file on disk.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Store {
    Dotenv,
    Yaml,
    Json,
    Ini,
}

impl Store {
    fn as_str(self) -> &'static str {
        match self {
            Store::Dotenv => "dotenv",
            Store::Yaml => "yaml",
            Store::Json => "json",
            Store::Ini => "ini",
        }
    }
}

/// A file's format: how it is stored, and whether its plaintext is the whole
/// file as text (sops' "binary" type) rather than a structure of values.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Format {
    store: Store,
    whole: bool,
}

impl Format {
    const DOTENV: Format = Format {
        store: Store::Dotenv,
        whole: false,
    };
    const BINARY: Format = Format {
        store: Store::Json,
        whole: true,
    };

    /// The name the UI shows and sops calls the plaintext type.
    fn name(self) -> &'static str {
        if self.whole {
            "binary"
        } else {
            self.store.as_str()
        }
    }

    /// Env files get the variable table; everything else a text editor.
    fn is_env(self) -> bool {
        self == Format::DOTENV
    }

    fn plain_type(self) -> &'static str {
        self.name()
    }

    /// Arguments for reading the file and printing its plaintext.
    fn decrypt_args(self) -> [&'static str; 4] {
        [
            "--input-type",
            self.store.as_str(),
            "--output-type",
            self.plain_type(),
        ]
    }

    /// Arguments for turning plaintext into a file of this format.
    fn encrypt_args(self) -> [&'static str; 4] {
        [
            "--input-type",
            self.plain_type(),
            "--output-type",
            self.store.as_str(),
        ]
    }

    /// Arguments for changing single values in place (`set`, `unset`).
    fn store_args(self) -> [&'static str; 4] {
        [
            "--input-type",
            self.store.as_str(),
            "--output-type",
            self.store.as_str(),
        ]
    }
}

/// The format a new file gets from its name: the extensions sops knows, env
/// files by an `env` part in the name, and anything else as text.
fn format_for_name(name: &str) -> Format {
    let lower = name.to_lowercase();
    let store = match lower.rsplit('.').next() {
        _ if !lower.contains('.') => None,
        Some("yaml" | "yml") => Some(Store::Yaml),
        Some("json") => Some(Store::Json),
        Some("ini") => Some(Store::Ini),
        _ => None,
    };
    match store {
        Some(store) => Format {
            store,
            whole: false,
        },
        None if looks_like_env(name) => Format::DOTENV,
        None => Format::BINARY,
    }
}

fn looks_like_env(name: &str) -> bool {
    name.to_lowercase().split('.').any(|part| part == "env")
}

/// What a file's sops metadata says, as far as the app needs it.
#[derive(Clone, Debug, PartialEq)]
struct Meta {
    format: Format,
    /// age recipients, in the order they appear.
    recipients: Vec<String>,
    /// Also encrypted for PGP, a cloud KMS or Vault, which the app can't
    /// name when it encrypts the file again.
    other_keys: bool,
    /// Options to pass on when encrypting the file again, as sops flags.
    options: Vec<(&'static str, String)>,
    /// Set when the file uses something re-encrypting can't keep, with what.
    cant_keep: Option<&'static str>,
}

impl Meta {
    /// Why the app can't encrypt this file again without losing something,
    /// or `None` when it can.
    fn reencrypt_problem(&self) -> Option<String> {
        if self.other_keys {
            return Some(
                "This file is also encrypted for keys other than age, such as PGP or a cloud KMS. \
                 The app can't encrypt it again without them; change it with sops directly."
                    .into(),
            );
        }
        if let Some(what) = self.cant_keep {
            return Some(format!(
                "This file uses {what}, which the app can't keep when it encrypts the file again. \
                 Change it with sops directly."
            ));
        }
        if self.recipients.is_empty() {
            return Some(
                "This file isn't encrypted for an age key, so the app can't encrypt it again."
                    .into(),
            );
        }
        None
    }
}

/// What the UI sends as a file's content: variables for env files, the
/// whole text for every other format.
enum Content {
    Items(Vec<Item>),
    Text(String),
}

impl Content {
    fn from_body(body: &Value) -> Res<Content> {
        if let Some(text) = body.get("text").and_then(Value::as_str) {
            return Ok(Content::Text(text.to_string()));
        }
        serde_json::from_value(body.get("items").cloned().unwrap_or(Value::Null))
            .map(Content::Items)
            .map_err(|_| "The file content couldn't be read.".to_string())
    }
}

/// Metadata entries the app passes on, with the flag that sets them.
const KEPT_OPTIONS: [(&str, &str); 4] = [
    ("encrypted_regex", "--encrypted-regex"),
    ("unencrypted_regex", "--unencrypted-regex"),
    ("encrypted_suffix", "--encrypted-suffix"),
    ("unencrypted_suffix", "--unencrypted-suffix"),
];
/// Metadata entries sops only takes from .sops.yaml, not from flags.
const CONFIG_ONLY_OPTIONS: [&str; 3] = [
    "mac_only_encrypted",
    "encrypted_comment_regex",
    "unencrypted_comment_regex",
];
fn config_only_label(name: &str) -> &'static str {
    match name {
        "mac_only_encrypted" => "the sops option mac_only_encrypted",
        "encrypted_comment_regex" => "the sops option encrypted_comment_regex",
        _ => "the sops option unencrypted_comment_regex",
    }
}

const OTHER_KEY_TYPES: [&str; 5] = ["pgp", "kms", "gcp_kms", "azure_kv", "hc_vault"];

/// Recognises a sops-encrypted file by its content. `None` for anything else,
/// including files that merely mention sops.
fn detect(text: &str) -> Option<Meta> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    if text.lines().any(|l| l.starts_with("sops_mac=")) {
        let entries: Vec<(&str, &str)> = text
            .lines()
            .filter_map(|l| l.split_once('='))
            .filter_map(|(k, v)| Some((k.strip_prefix("sops_")?, v)))
            .collect();
        let only_data = text
            .lines()
            .filter(|l| !l.trim().is_empty() && !l.starts_with('#') && !l.starts_with("sops_"))
            .all(|l| l.starts_with("data="));
        let format = Format {
            store: Store::Dotenv,
            whole: only_data && text.lines().any(|l| l.starts_with("data=")),
        };
        return Some(flat_meta(format, &entries));
    }
    if let Some(start) = text.lines().position(|l| l.trim() == "[sops]") {
        let entries: Vec<(&str, &str)> = text
            .lines()
            .skip(start + 1)
            .take_while(|l| !l.trim_start().starts_with('['))
            .filter_map(|l| l.split_once('='))
            .map(|(k, v)| (k.trim(), v.trim()))
            .collect();
        if entries.iter().any(|(k, _)| *k == "mac") {
            let format = Format {
                store: Store::Ini,
                whole: false,
            };
            return Some(flat_meta(format, &entries));
        }
        return None;
    }
    if text.trim_start().starts_with('{') {
        let doc: Value = serde_json::from_str(text).ok()?;
        let top = doc.as_object()?;
        let keys: Vec<&str> = top.keys().map(String::as_str).collect();
        return tree_meta(Store::Json, top.get("sops")?, &keys, top.get("data"));
    }
    // YAML, possibly with several documents; sops writes its metadata into each.
    use serde::Deserialize as _;
    for doc in serde_yaml::Deserializer::from_str(text) {
        let Ok(serde_yaml::Value::Mapping(map)) = serde_yaml::Value::deserialize(doc) else {
            continue;
        };
        let Some(sops) = map.get("sops") else {
            continue;
        };
        let sops = serde_json::to_value(sops).ok()?;
        let keys: Vec<&str> = map.keys().filter_map(|k| k.as_str()).collect();
        let data = map.get("data").and_then(|d| serde_json::to_value(d).ok());
        return tree_meta(Store::Yaml, &sops, &keys, data.as_ref());
    }
    None
}

/// Metadata from dotenv and INI files, where sops flattens it into keys
/// such as `age__list_0__map_recipient`.
fn flat_meta(format: Format, entries: &[(&str, &str)]) -> Meta {
    let recipient = Regex::new(r"(^|__)age__list_\d+__map_recipient$").unwrap();
    let other = Regex::new(&format!(r"(^|__)({})__list_", OTHER_KEY_TYPES.join("|"))).unwrap();
    let groups: BTreeSet<&str> = entries
        .iter()
        .filter_map(|(k, _)| k.strip_prefix("key_groups__list_"))
        .filter_map(|rest| rest.split("__").next())
        .collect();
    let value = |name: &str| {
        entries
            .iter()
            .find(|(k, _)| *k == name)
            .map(|(_, v)| v.trim().to_string())
    };
    let config_only = CONFIG_ONLY_OPTIONS
        .iter()
        .find(|name| value(name).is_some_and(|v| !v.is_empty() && v != "false"));
    Meta {
        format,
        recipients: entries
            .iter()
            .filter(|(k, _)| recipient.is_match(k))
            .map(|(_, v)| v.trim().to_string())
            .filter(|v| v.starts_with("age1"))
            .collect(),
        other_keys: entries.iter().any(|(k, _)| other.is_match(k)),
        options: KEPT_OPTIONS
            .iter()
            .filter_map(|(name, flag)| Some((*flag, value(name)?)))
            .collect(),
        cant_keep: if groups.len() > 1 {
            Some("several key groups")
        } else {
            config_only.map(|name| config_only_label(name))
        },
    }
}

/// Metadata from YAML and JSON files, where it is a `sops` object.
fn tree_meta(store: Store, sops: &Value, keys: &[&str], data: Option<&Value>) -> Option<Meta> {
    sops.get("mac")?.as_str()?;
    let list = |v: Option<&Value>| v.and_then(Value::as_array).cloned().unwrap_or_default();
    let groups = list(sops.get("key_groups"));
    let mut places = vec![sops];
    places.extend(groups.iter());
    let recipients = places
        .iter()
        .flat_map(|p| list(p.get("age")))
        .filter_map(|r| r.get("recipient")?.as_str().map(str::to_string))
        .collect();
    let other_keys = places
        .iter()
        .any(|p| OTHER_KEY_TYPES.iter().any(|t| !list(p.get(*t)).is_empty()));
    let set = |name: &str| match sops.get(name) {
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Bool(b)) => *b,
        _ => false,
    };
    let whole =
        data.is_some_and(Value::is_string) && keys.iter().all(|k| matches!(*k, "data" | "sops"));
    Some(Meta {
        format: Format { store, whole },
        recipients,
        other_keys,
        options: KEPT_OPTIONS
            .iter()
            .filter_map(|(name, flag)| Some((*flag, sops.get(*name)?.as_str()?.to_string())))
            .collect(),
        cant_keep: if groups.len() > 1 {
            Some("several key groups")
        } else {
            CONFIG_ONLY_OPTIONS
                .iter()
                .find(|n| set(n))
                .map(|name| config_only_label(name))
        },
    })
}

/// A cheap first look at the end of a file, where sops puts its metadata:
/// files without an encrypted MAC there are skipped without reading them.
const TAIL_BYTES: u64 = 64 * 1024;

fn tail_looks_encrypted(path: &Path, len: u64) -> bool {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let start = len.saturating_sub(TAIL_BYTES);
    if file.seek(SeekFrom::Start(start)).is_err() {
        return false;
    }
    let mut buf = Vec::new();
    if file.take(TAIL_BYTES).read_to_end(&mut buf).is_err() {
        return false;
    }
    let has = |needle: &[u8]| buf.windows(needle.len()).any(|w| w == needle);
    has(b"ENC[AES256_GCM,data:") && has(b"mac")
}

/// Extensions of files that are never text, so never sops files.
const BINARY_EXTENSIONS: [&str; 32] = [
    "png", "jpg", "jpeg", "gif", "webp", "ico", "bmp", "tif", "tiff", "pdf", "zip", "gz", "tgz",
    "bz2", "xz", "7z", "rar", "tar", "jar", "war", "class", "so", "dll", "dylib", "exe", "o", "a",
    "woff", "woff2", "ttf", "mp4", "mp3",
];

// ---------------------------------------------------------------------------
// .sops.yaml rules
// ---------------------------------------------------------------------------

fn find_sops_yaml(file: &Path) -> Option<PathBuf> {
    file.parent()?
        .ancestors()
        .map(|d| d.join(".sops.yaml"))
        .find(|c| c.is_file())
}

/// The age recipients the nearest .sops.yaml prescribes for this file, or
/// `None` when there is no config or no matching rule.
fn rule_recipients(file: &Path) -> Option<Vec<String>> {
    let cfg_path = find_sops_yaml(file)?;
    let cfg: serde_yaml::Value =
        serde_yaml::from_str(&std::fs::read_to_string(&cfg_path).ok()?).ok()?;
    let rel = file
        .strip_prefix(cfg_path.parent()?)
        .ok()?
        .to_string_lossy()
        .replace('\\', "/");
    fn collect(v: Option<&serde_yaml::Value>, out: &mut Vec<String>) {
        match v {
            Some(serde_yaml::Value::String(s)) => out.extend(
                s.split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string),
            ),
            Some(serde_yaml::Value::Sequence(seq)) => {
                seq.iter().for_each(|x| collect(Some(x), out))
            }
            _ => {}
        }
    }
    for rule in cfg.get("creation_rules")?.as_sequence()? {
        if let Some(pattern) = rule.get("path_regex").and_then(|v| v.as_str()) {
            if !Regex::new(pattern)
                .map(|re| re.is_match(&rel))
                .unwrap_or(false)
            {
                continue;
            }
        }
        let mut keys = Vec::new();
        collect(rule.get("age"), &mut keys);
        if let Some(groups) = rule.get("key_groups").and_then(|g| g.as_sequence()) {
            groups.iter().for_each(|g| collect(g.get("age"), &mut keys));
        }
        return Some(keys);
    }
    None
}

// ---------------------------------------------------------------------------
// Git (optional)
// ---------------------------------------------------------------------------
//
// Env files usually live in Git repositories, so the app groups them by
// repository and shows what changed since the last commit. None of this is
// required: a folder without a repository, or a machine without git, simply
// gets less information.

/// The repository a file belongs to: the nearest folder above it holding a
/// `.git` entry. A plain file system lookup, so it works without git.
fn repo_root(file: &Path) -> Option<PathBuf> {
    file.parent()?
        .ancestors()
        .find(|dir| dir.join(".git").exists())
        .map(PathBuf::from)
}

/// The checked-out branch, read from `.git/HEAD`. In a worktree or submodule
/// `.git` is a file pointing at the real folder.
fn repo_branch(root: &Path) -> Option<String> {
    let dot_git = root.join(".git");
    let git_dir = if dot_git.is_file() {
        let pointer = std::fs::read_to_string(&dot_git).ok()?;
        root.join(
            pointer
                .lines()
                .find_map(|l| l.strip_prefix("gitdir:"))?
                .trim(),
        )
    } else {
        dot_git
    };
    let head = std::fs::read_to_string(git_dir.join("HEAD")).ok()?;
    let head = head.trim();
    Some(match head.strip_prefix("ref: refs/heads/") {
        Some(branch) => branch.to_string(),
        None => head.chars().take(7).collect(), // detached: short commit id
    })
}

/// A file's path inside its repository, with forward slashes as git expects.
fn repo_relative(root: &Path, file: &Path) -> Option<String> {
    Some(
        file.strip_prefix(root)
            .ok()?
            .to_string_lossy()
            .replace('\\', "/"),
    )
}

/// Runs a read-only git command in a repository that may not be trustworthy.
///
/// A repository's own config can name programs for git to run, such as
/// `core.fsmonitor` or hooks. Without these options, opening a folder that
/// contains a prepared repository would execute them. None of the options
/// changes what the commands report.
fn git_read(git: &Path, root: &Path, args: &[&str]) -> std::io::Result<Output> {
    let no_hooks = neutral_dir()
        .map(|d| d.join("no-git-hooks").to_string_lossy().into_owned())
        .unwrap_or_else(|| "no-git-hooks".into());
    let hooks = format!("core.hooksPath={no_hooks}");
    let mut all = vec![
        "--no-optional-locks", // don't rewrite the index, which could also fire a hook
        "--no-pager",
        "-c",
        "core.fsmonitor=false",
        "-c",
        hooks.as_str(),
    ];
    all.extend(args);
    run(
        git,
        &all,
        &[("GIT_TERMINAL_PROMPT", "0".into())],
        Some(root),
        None,
    )
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum GitState {
    Clean,
    Modified,
    New,
    Ignored,
}

impl GitState {
    fn as_str(self) -> &'static str {
        match self {
            GitState::Clean => "clean",
            GitState::Modified => "modified",
            GitState::New => "new",
            GitState::Ignored => "ignored",
        }
    }
}

/// Asks git about a set of files in one repository: two calls per repository,
/// not per file. Returns `None` when git can't answer.
fn git_states(git: &Path, root: &Path, files: &[PathBuf]) -> Option<BTreeMap<PathBuf, GitState>> {
    let rels: Vec<String> = files
        .iter()
        .filter_map(|f| repo_relative(root, f))
        .collect();
    let call = |args: &[&str]| -> Option<String> {
        let mut all: Vec<&str> = args.to_vec();
        all.push("--");
        all.extend(rels.iter().map(String::as_str));
        let out = git_read(git, root, &all).ok()?;
        (out.code == 0).then_some(out.stdout)
    };
    let tracked: BTreeSet<String> = call(&["ls-files", "-z"])?
        .split('\0')
        .filter(|p| !p.is_empty())
        .map(str::to_string)
        .collect();
    // Entries look like "XY path"; a rename is followed by its old path.
    let mut changed: BTreeMap<String, GitState> = BTreeMap::new();
    let status = call(&[
        "status",
        "--porcelain=v1",
        "-z",
        "--untracked-files=all",
        "--ignore-submodules=all",
    ])?;
    let mut entries = status.split('\0');
    while let Some(entry) = entries.next() {
        if entry.len() < 4 {
            continue;
        }
        let (code, path) = (&entry[..2], &entry[3..]);
        if code.contains('R') || code.contains('C') {
            entries.next();
        }
        let state = if code == "??" || code.starts_with('A') {
            GitState::New
        } else {
            GitState::Modified
        };
        changed.insert(path.to_string(), state);
    }
    let mut states = BTreeMap::new();
    for file in files {
        let Some(rel) = repo_relative(root, file) else {
            continue;
        };
        let state = match changed.get(&rel) {
            Some(state) => *state,
            None if tracked.contains(&rel) => GitState::Clean,
            // Untracked and not reported as such: an ignore rule covers it.
            None => GitState::Ignored,
        };
        states.insert(file.clone(), state);
    }
    Some(states)
}

/// What changed between two versions of an env file, by variable.
fn env_changes(old: &str, new: &str) -> (Vec<Value>, usize) {
    let pairs = |text: &str| -> Vec<(String, String)> {
        parse_env(text)
            .into_iter()
            .filter_map(|item| match item {
                Item::Kv { k, v } => Some((k, v)),
                Item::Raw { .. } => None,
            })
            .collect()
    };
    let (old, new) = (pairs(old), pairs(new));
    let find = |list: &[(String, String)], key: &str| {
        list.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
    };
    let mut changes = Vec::new();
    let mut unchanged = 0;
    for (k, v) in &new {
        match find(&old, k) {
            None => changes.push(json!({ "k": k, "kind": "added", "new": v })),
            Some(before) if before != *v => {
                changes.push(json!({ "k": k, "kind": "changed", "old": before, "new": v }))
            }
            Some(_) => unchanged += 1,
        }
    }
    for (k, v) in &old {
        if find(&new, k).is_none() {
            changes.push(json!({ "k": k, "kind": "removed", "old": v }));
        }
    }
    (changes, unchanged)
}

/// What changed between two versions of a text file, line by line: changed
/// lines with up to two unchanged lines around them, and the number of lines
/// left out in between (`skip`). Returns the lines and how many changed.
fn text_changes(old: &str, new: &str) -> (Vec<Value>, usize) {
    const CONTEXT: usize = 2;
    let split = |t: &str| -> Vec<String> {
        t.replace("\r\n", "\n")
            .lines()
            .map(str::to_string)
            .collect()
    };
    let (a, b) = (split(old), split(new));
    let prefix = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let suffix = a[prefix..]
        .iter()
        .rev()
        .zip(b[prefix..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let (mid_a, mid_b) = (&a[prefix..a.len() - suffix], &b[prefix..b.len() - suffix]);
    // Each line as (kind, text): ' ' unchanged, '-' removed, '+' added.
    let mut ops: Vec<(char, &str)> = a[..prefix].iter().map(|l| (' ', l.as_str())).collect();
    if mid_a.len().saturating_mul(mid_b.len()) <= 4_000_000 {
        // Longest common subsequence of the lines that differ.
        let (n, m) = (mid_a.len(), mid_b.len());
        let mut lcs = vec![vec![0u32; m + 1]; n + 1];
        for i in (0..n).rev() {
            for j in (0..m).rev() {
                lcs[i][j] = if mid_a[i] == mid_b[j] {
                    lcs[i + 1][j + 1] + 1
                } else {
                    lcs[i + 1][j].max(lcs[i][j + 1])
                };
            }
        }
        let (mut i, mut j) = (0, 0);
        while i < n || j < m {
            if i < n && j < m && mid_a[i] == mid_b[j] {
                ops.push((' ', &mid_a[i]));
                i += 1;
                j += 1;
            } else if i < n && (j == m || lcs[i + 1][j] >= lcs[i][j + 1]) {
                // Removed lines come before the lines that replace them.
                ops.push(('-', &mid_a[i]));
                i += 1;
            } else {
                ops.push(('+', &mid_b[j]));
                j += 1;
            }
        }
    } else {
        // Too big to compare line by line: show it as replaced.
        ops.extend(mid_a.iter().map(|l| ('-', l.as_str())));
        ops.extend(mid_b.iter().map(|l| ('+', l.as_str())));
    }
    ops.extend(a[a.len() - suffix..].iter().map(|l| (' ', l.as_str())));
    let changed: Vec<usize> = (0..ops.len()).filter(|&i| ops[i].0 != ' ').collect();
    let near = |i: usize| changed.iter().any(|&c| c.abs_diff(i) <= CONTEXT);
    let mut lines = Vec::new();
    let mut skipped = 0;
    for (i, (kind, text)) in ops.iter().enumerate() {
        if *kind == ' ' && !near(i) {
            skipped += 1;
            continue;
        }
        if skipped > 0 {
            lines.push(json!({ "kind": "skip", "count": skipped }));
            skipped = 0;
        }
        let kind = match kind {
            '+' => "added",
            '-' => "removed",
            _ => "same",
        };
        lines.push(json!({ "kind": kind, "text": text }));
    }
    if skipped > 0 && !changed.is_empty() {
        lines.push(json!({ "kind": "skip", "count": skipped }));
    }
    (lines, changed.len())
}

// ---------------------------------------------------------------------------
// The app
// ---------------------------------------------------------------------------

/// Folders the scan doesn't enter unless the settings say otherwise:
/// dependencies and build output, which never hold a project's own secrets.
/// Hidden folders (`.git`, `.venv` …) are always skipped.
pub const DEFAULT_SKIP_DIRS: [&str; 7] = [
    "node_modules",
    "vendor",
    "dist",
    "build",
    "target",
    "venv",
    "__pycache__",
];

/// How many folder levels below the open folder are searched, unless set.
pub const DEFAULT_SCAN_DEPTH: u32 = 3;
pub const MAX_SCAN_DEPTH: u32 = 10;
/// At most this many files are listed, and at most this many folder entries
/// looked at, so a huge folder (a home directory) can't stall the app.
const MAX_LISTED: usize = 200;
const MAX_SCANNED_ENTRIES: usize = 20_000;

/// What the scan learned about one file, kept until its size or time changes.
#[derive(Clone)]
struct Seen {
    len: u64,
    modified: Option<std::time::SystemTime>,
    /// `None` when the file isn't a sops file.
    found: Option<Found>,
}

#[derive(Clone)]
struct Found {
    rev: String,
    meta: Meta,
}

/// Results of earlier scans by path. Refreshing reads only files that are new
/// or changed since, so a refresh costs about as much as listing the folders.
#[derive(Clone, Default)]
pub struct ScanCache(std::sync::Arc<BTreeMap<PathBuf, Seen>>);

/// Everything a scan needs, copied out of the app so that it can run without
/// holding the app's lock: opening and saving files go on meanwhile.
pub struct ScanJob {
    folder: PathBuf,
    depth: u32,
    skip: Vec<String>,
    extra: Vec<PathBuf>,
    cache: ScanCache,
    git: Option<PathBuf>,
}

/// A file as the list shows it, before the keys are taken into account.
struct Listed {
    path: PathBuf,
    rev: Option<String>,
    meta: Option<Meta>,
    /// Status and note for a file that can't be listed normally.
    problem: Option<(&'static str, String)>,
    drift: Option<Value>,
}

pub struct ScanResult {
    folder: PathBuf,
    files: Vec<Listed>,
    states: BTreeMap<PathBuf, GitState>,
    repos: Vec<Value>,
    limited: bool,
    cache: ScanCache,
}

/// Walks the folder tree. Hidden folders and the usual dependency and build
/// folders are skipped; files go to `out` in a stable order.
fn walk(
    dir: &Path,
    depth: u32,
    skip: &[String],
    out: &mut Vec<(PathBuf, std::fs::Metadata)>,
    budget: &mut usize,
) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = read.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        if *budget == 0 {
            return;
        }
        *budget -= 1;
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        // Follows symbolic links, like the file list always did.
        let Ok(meta) = std::fs::metadata(&path) else {
            continue;
        };
        if meta.is_file() {
            out.push((path, meta));
        } else if meta.is_dir()
            && depth > 0
            && !name.starts_with('.')
            && !skip.iter().any(|s| s == &name)
        {
            walk(&path, depth - 1, skip, out, budget);
        }
    }
}

/// Whether a file is worth opening at all: small enough, not empty, and not
/// of a type that is never text.
fn worth_reading(path: &Path, len: u64) -> bool {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    len > 0 && len <= MAX_FILE_BYTES && !BINARY_EXTENSIONS.contains(&ext.as_str())
}

/// Reads a file and recognises it as a sops file, or not.
fn inspect(path: &Path, len: u64) -> Option<Found> {
    if !worth_reading(path, len) || !tail_looks_encrypted(path, len) {
        return None;
    }
    let file = read_text(path).ok()?;
    Some(Found {
        meta: detect(&file.text)?,
        rev: file.rev,
    })
}

impl ScanJob {
    pub fn run(self) -> ScanResult {
        let mut entries = Vec::new();
        let mut budget = MAX_SCANNED_ENTRIES;
        walk(
            &self.folder,
            self.depth,
            &self.skip,
            &mut entries,
            &mut budget,
        );
        let limited = budget == 0;
        let mut cache = BTreeMap::new();
        let mut files = Vec::new();
        let mut look = |path: &Path, meta: &std::fs::Metadata| -> Option<Found> {
            let (len, modified) = (meta.len(), meta.modified().ok());
            let seen = match self.cache.0.get(path) {
                Some(s) if s.len == len && s.modified == modified && modified.is_some() => {
                    s.clone()
                }
                _ => Seen {
                    len,
                    modified,
                    found: inspect(path, len),
                },
            };
            let found = seen.found.clone();
            cache.insert(path.to_path_buf(), seen);
            found
        };
        for (path, meta) in &entries {
            if files.len() >= MAX_LISTED {
                break;
            }
            if let Some(found) = look(path, meta) {
                files.push(Listed {
                    drift: drift(path, &found.meta.recipients),
                    path: path.clone(),
                    rev: Some(found.rev),
                    meta: Some(found.meta),
                    problem: None,
                });
            }
        }
        // Files opened by hand stay listed, and say so when something is wrong.
        for extra in &self.extra {
            if files.iter().any(|f| &f.path == extra) {
                continue;
            }
            let mut listed = Listed {
                path: extra.clone(),
                rev: None,
                meta: None,
                problem: None,
                drift: None,
            };
            match std::fs::metadata(extra) {
                Err(_) => listed.problem = Some(("missing", "File not found".into())),
                Ok(meta) => match look(extra, &meta) {
                    Some(found) => {
                        listed.drift = drift(extra, &found.meta.recipients);
                        listed.rev = Some(found.rev);
                        listed.meta = Some(found.meta);
                    }
                    None => listed.problem = Some(("unreadable", "Not encrypted with SOPS".into())),
                },
            }
            files.push(listed);
        }
        // Group the files by repository, then ask git once per repository.
        let mut by_repo: BTreeMap<PathBuf, Vec<PathBuf>> = BTreeMap::new();
        for file in &files {
            if let Some(root) = repo_root(&file.path) {
                by_repo.entry(root).or_default().push(file.path.clone());
            }
        }
        let mut states = BTreeMap::new();
        let mut repos = Vec::new();
        for (root, paths) in &by_repo {
            if let Some(found) = self
                .git
                .as_ref()
                .and_then(|git| git_states(git, root, paths))
            {
                states.extend(found);
            }
            repos.push(json!({
                "root": root.to_string_lossy(),
                "name": root.file_name().map(|n| n.to_string_lossy().into_owned()),
                "branch": repo_branch(root),
            }));
        }
        ScanResult {
            folder: self.folder,
            files,
            states,
            repos,
            limited,
            cache: ScanCache(std::sync::Arc::new(cache)),
        }
    }
}

/// How a file's recipients differ from what its .sops.yaml rule prescribes.
fn drift(file: &Path, recipients: &[String]) -> Option<Value> {
    let expected = rule_recipients(file)?;
    let missing: Vec<_> = expected
        .iter()
        .filter(|k| !recipients.contains(k))
        .collect();
    let extra: Vec<_> = recipients
        .iter()
        .filter(|k| !expected.contains(k))
        .collect();
    (!missing.is_empty() || !extra.is_empty())
        .then(|| json!({ "missing": missing, "extra": extra }))
}

/// The newest release worth announcing to someone running `current`, from a
/// list of releases as the GitHub API returns them.
///
/// A release counts when it's newer than `current`. Someone on a stable
/// version only hears about stable releases; someone on a pre-release also
/// hears about newer pre-releases, and about the stable release that follows.
/// Drafts and tags that aren't versions are ignored.
pub fn pick_update(current: &str, releases: &Value) -> Option<Value> {
    let current = semver::Version::parse(current.trim_start_matches('v')).ok()?;
    let on_prerelease = !current.pre.is_empty();
    releases
        .as_array()?
        .iter()
        .filter(|r| r["draft"] != true)
        .filter_map(|r| {
            let tag = r["tag_name"].as_str()?;
            let version = semver::Version::parse(tag.strip_prefix('v')?).ok()?;
            let prerelease = !version.pre.is_empty() || r["prerelease"] == true;
            (version > current && (on_prerelease || !prerelease))
                .then_some((version, tag, r, prerelease))
        })
        .max_by(|a, b| a.0.cmp(&b.0))
        .map(|(version, tag, r, prerelease)| {
            json!({
                "version": version.to_string(),
                "tag": tag,
                "prerelease": prerelease,
                "notes": r["body"].as_str().unwrap_or(""),
                "published": r["published_at"],
            })
        })
}

/// Release tags as GitHub URLs may contain them; anything else is refused.
pub fn valid_release_tag(tag: &str) -> bool {
    tag.strip_prefix('v')
        .is_some_and(|v| semver::Version::parse(v).is_ok())
}

pub struct App {
    config: Config,
    session_keys: Vec<KeySource>,
    key_sources: Vec<KeySource>,
    folder: PathBuf,
    extra_files: Vec<PathBuf>,
    sops: SopsInfo,
    git: Option<PathBuf>,
    bundled_dirs: Vec<PathBuf>,
    next_id: u32,
    /// The version of this build, shown in the app and compared with releases.
    version: String,
    /// A key pair created for a server or CI, waiting for its private key to be
    /// copied (`machine_key_secret`). Held in memory only; dropped once the
    /// public key is saved as a recipient or the user discards it.
    machine_key: Option<(String, Identity)>,
    scan_cache: ScanCache,
}

impl App {
    /// `bundled_dirs` are searched for a shipped sops binary before PATH.
    pub fn new(folder: Option<PathBuf>, bundled_dirs: Vec<PathBuf>) -> Self {
        let config = load_config();
        let folder = folder
            .or_else(|| {
                config
                    .last_folder
                    .as_ref()
                    .map(PathBuf::from)
                    .filter(|p| p.is_dir())
            })
            .unwrap_or_else(home_dir);
        let mut app = Self {
            sops: detect_sops(&bundled_dirs),
            git: find_program("git"),
            config,
            session_keys: vec![],
            key_sources: vec![],
            folder,
            extra_files: vec![],
            bundled_dirs,
            next_id: 0,
            version: env!("CARGO_PKG_VERSION").to_string(),
            machine_key: None,
            scan_cache: ScanCache::default(),
        };
        app.remember_folder();
        app.reload_keys();
        app
    }

    /// The private key of the key pair made with `machineKeyCreate`, for the
    /// desktop shell to put on the clipboard. It never goes to the page.
    pub fn machine_key_secret(&self) -> Option<String> {
        self.machine_key.as_ref().map(|(_, id)| id.secret.clone())
    }

    /// Seconds after which a copied value is removed from the clipboard; 0 keeps it.
    /// Sets the version of this build (from the release tag); see scripts/version.sh.
    pub fn set_version(&mut self, version: &str) {
        self.version = version.to_string();
    }

    pub fn scan_depth(&self) -> u32 {
        self.config
            .scan_depth
            .unwrap_or(DEFAULT_SCAN_DEPTH)
            .clamp(1, MAX_SCAN_DEPTH)
    }

    pub fn skip_dirs(&self) -> Vec<String> {
        self.config
            .skip_dirs
            .clone()
            .unwrap_or_else(|| DEFAULT_SKIP_DIRS.iter().map(|d| d.to_string()).collect())
    }

    pub fn check_updates(&self) -> bool {
        self.config.check_updates.unwrap_or(true)
    }

    pub fn clipboard_clear_seconds(&self) -> u32 {
        self.config
            .clipboard_clear_seconds
            .unwrap_or(DEFAULT_CLIPBOARD_CLEAR_SECONDS)
    }

    /// Puts the current folder at the front of the recent list.
    fn remember_folder(&mut self) {
        let current = self.folder.to_string_lossy().into_owned();
        self.config.recent.retain(|f| *f != current);
        self.config.recent.insert(0, current);
        self.config.recent.truncate(8);
    }

    fn save_config(&self) -> Res<()> {
        let path = config_path();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(io_msg)?;
        }
        let text = serde_json::to_string_pretty(&self.config).map_err(|e| e.to_string())?;
        std::fs::write(path, text).map_err(io_msg)
    }

    fn reload_keys(&mut self) {
        let default = default_key_path();
        let (identities, error) = read_key_file(&default);
        let mut list = vec![KeySource {
            id: "default".into(),
            kind: "default",
            label: "default".into(),
            path: Some(default.to_string_lossy().into_owned()),
            identities,
            error,
        }];
        for kp in &self.config.key_paths {
            let (identities, error) = read_key_file(Path::new(&kp.path));
            list.push(KeySource {
                id: format!("path:{}", kp.path),
                kind: "path",
                label: kp.label.clone(),
                path: Some(kp.path.clone()),
                identities,
                error,
            });
        }
        list.extend(self.session_keys.iter().cloned());
        self.key_sources = list;
    }

    fn all_secrets(&self) -> String {
        let mut seen = BTreeSet::new();
        self.key_sources
            .iter()
            .flat_map(|s| &s.identities)
            .filter(|i| seen.insert(i.public.clone()))
            .map(|i| i.secret.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn label_for(&self, public: &str) -> Option<&str> {
        self.key_sources
            .iter()
            .find(|s| s.identities.iter().any(|i| i.public == public))
            .map(|s| s.label.as_str())
    }

    fn name_for(&self, public: &str) -> String {
        if let Some(label) = self.label_for(public) {
            return format!("You ({label})");
        }
        if let Some(entry) = self.config.book.iter().find(|b| b.key == public) {
            return entry.name.clone();
        }
        format!(
            "{}…{}",
            &public[..public.len().min(10)],
            &public[public.len().saturating_sub(4)..]
        )
    }

    fn new_session_key(
        &mut self,
        label: &str,
        fallback: &str,
        identities: Vec<Identity>,
    ) -> String {
        self.next_id += 1;
        let id = format!("session:{}", self.next_id);
        let label = if label.trim().is_empty() {
            fallback
        } else {
            label.trim()
        };
        self.session_keys.push(KeySource {
            id: id.clone(),
            kind: "session",
            label: label.to_string(),
            path: None,
            identities,
            error: None,
        });
        id
    }

    // ---- sops ------------------------------------------------------------

    fn sops(
        &self,
        args: &[&str],
        extra_env: &[(&str, String)],
        cwd: Option<&Path>,
        stdin: Option<&str>,
    ) -> Res<Output> {
        let missing = || "sops wasn't found next to the app or on your PATH.".to_string();
        let path = self.sops.path.as_ref().ok_or_else(missing)?;
        let mut env = Vec::new();
        let secrets = self.all_secrets();
        if !secrets.is_empty() {
            env.push(("SOPS_AGE_KEY", secrets));
        }
        env.extend(extra_env.iter().cloned());
        run(Path::new(path), args, &env, cwd, stdin).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                missing()
            } else {
                e.to_string()
            }
        })
    }

    /// Reads a file and its sops metadata; refuses files sops didn't encrypt.
    fn load(&self, file: &Path) -> Res<(TextFile, Meta)> {
        let on_disk = read_text(file).map_err(|p| p.message())?;
        let meta = detect(&on_disk.text)
            .ok_or("This file isn't encrypted with SOPS, so the app doesn't open it.")?;
        Ok((on_disk, meta))
    }

    fn decrypt(&self, file: &Path, format: Format) -> Res<String> {
        let path = file.to_string_lossy();
        let mut args = vec!["decrypt"];
        args.extend(format.decrypt_args());
        args.push(&path);
        let out = self.sops(&args, &[], neutral_dir().as_deref(), None)?;
        plaintext(out)
    }

    /// Decrypts encrypted content that isn't on disk, such as a committed version.
    fn decrypt_text(&self, encrypted: &str, format: Format) -> Res<String> {
        let mut args = vec!["decrypt"];
        args.extend(format.decrypt_args());
        args.extend(["--filename-override", "committed"]);
        let out = self.sops(&args, &[], neutral_dir().as_deref(), Some(encrypted))?;
        plaintext(out)
    }

    /// Encrypts plaintext from stdin as if it were `file`, which needn't exist.
    /// With `recipients == None` the matching .sops.yaml rule decides.
    fn encrypt_text(
        &self,
        file: &Path,
        plaintext: &str,
        format: Format,
        recipients: Option<&[String]>,
        options: &[(&'static str, String)],
    ) -> Res<String> {
        let path = file.to_string_lossy();
        let joined = recipients.map(|r| r.join(","));
        let mut args = vec!["encrypt"];
        if let Some(list) = &joined {
            args.extend(["--age", list.as_str()]);
        }
        for (flag, value) in options {
            args.extend([*flag, value.as_str()]);
        }
        args.extend(format.encrypt_args());
        args.extend(["--filename-override", &path]);
        let neutral = neutral_dir();
        let cwd = if recipients.is_some() {
            neutral.as_deref()
        } else {
            file.parent()
        };
        let out = self.sops(&args, &[], cwd, Some(plaintext))?;
        if out.code != 0 {
            return Err(sops_error(&out.stderr));
        }
        Ok(out.stdout)
    }

    /// Encrypts the whole file again for `recipients`, keeping the options
    /// recorded in its metadata (which values are encrypted). Only the result
    /// is written, after sops succeeded.
    fn reencrypt(
        &self,
        file: &Path,
        meta: &Meta,
        plaintext: &str,
        recipients: &[String],
    ) -> Res<()> {
        if let Some(problem) = meta.reencrypt_problem() {
            return Err(problem);
        }
        if recipients.is_empty() {
            return Err("A file needs at least one recipient.".into());
        }
        let encrypted = self.encrypt_text(
            file,
            plaintext,
            meta.format,
            Some(recipients),
            &meta.options,
        )?;
        std::fs::write(file, encrypted).map_err(io_msg)
    }

    /// Where a new file would go, after checking the folder and name. Nothing
    /// is written; `create` does that once there is content to encrypt.
    fn new_file_path(&self, dir: &str, name: &str) -> Res<PathBuf> {
        let dir = PathBuf::from(dir);
        if !dir.is_dir() {
            return Err("That folder doesn't exist.".into());
        }
        let name = name.trim();
        if name.is_empty() || name.contains(['/', '\\']) || name == "." || name == ".." {
            return Err("Enter a file name without folders, such as .env.production.".into());
        }
        let path = dir.join(name);
        if path.exists() {
            return Err(
                "A file with that name already exists. Open it instead, or choose another name."
                    .into(),
            );
        }
        Ok(path)
    }

    /// Turns what the UI sends into the plaintext sops gets for `format`.
    fn plaintext_for(format: Format, content: &Content) -> Res<String> {
        match (format.is_env(), content) {
            (true, Content::Items(items)) => {
                if !items.iter().any(|i| matches!(i, Item::Kv { .. })) {
                    return Err("Add at least one variable.".into());
                }
                serialize_env(items)
            }
            (false, Content::Text(text)) if text.trim().is_empty() => {
                Err("Enter the file's content.".into())
            }
            (false, Content::Text(text)) => Ok(text.clone()),
            _ => Err("The file content couldn't be read.".into()),
        }
    }

    /// Creates a new encrypted file. The content goes to sops through stdin
    /// and only the encrypted result is written, to a file that must not
    /// exist yet: an existing file is never replaced.
    fn create_file(
        &mut self,
        dir: &str,
        name: &str,
        content: &Content,
        recipients: &[String],
    ) -> Res<PathBuf> {
        let path = self.new_file_path(dir, name)?;
        let format = format_for_name(name.trim());
        let plaintext = Self::plaintext_for(format, content)?;
        let encrypted = match rule_recipients(&path) {
            Some(_) => self.encrypt_text(&path, &plaintext, format, None, &[])?,
            None if recipients.is_empty() => {
                return Err("Choose at least one recipient to encrypt for.".into())
            }
            None => {
                if recipients.iter().any(|k| !valid_public_key(k)) {
                    return Err("That isn't a valid age public key.".into());
                }
                self.encrypt_text(&path, &plaintext, format, Some(recipients), &[])?
            }
        };
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::AlreadyExists {
                    "A file with that name was just created by something else. Choose another name."
                        .to_string()
                } else {
                    io_msg(e)
                }
            })?;
        file.write_all(encrypted.as_bytes()).map_err(io_msg)?;
        // Deeper than the folder scan reaches, or outside the folder: keep it listed.
        if !self.extra_files.contains(&path) {
            self.extra_files.push(path.clone());
        }
        Ok(path)
    }

    /// Saves new content to an encrypted file. `opened_rev` is the fingerprint
    /// the file had when the UI loaded it; if the file has changed since (a
    /// `git pull`, another editor), nothing is written and `Ok(false)` reports
    /// the conflict, unless `overwrite` says the user confirmed.
    fn save_file(
        &self,
        file: &Path,
        content: &Content,
        opened_rev: Option<&str>,
        overwrite: bool,
    ) -> Res<bool> {
        let (on_disk, meta) = self.load(file)?;
        let plaintext = Self::plaintext_for(meta.format, content)?;
        if !overwrite && opened_rev.is_some_and(|rev| rev != on_disk.rev) {
            return Ok(false);
        }
        let path = file.to_string_lossy();
        let neutral = neutral_dir();
        let before = self.decrypt(file, meta.format)?;
        // Every step goes through stdin, so nothing decrypted touches the disk
        // (`sops edit` would write a temp file). If one fails, the file gets
        // back exactly the bytes it had.
        let set = |index: &str, value: &str| -> Res<()> {
            // sops expects the value JSON-encoded.
            let value = serde_json::to_string(value).map_err(|e| e.to_string())?;
            let mut args = vec!["set", "--value-stdin"];
            args.extend(meta.format.store_args());
            args.extend([&*path, index]);
            let out = self.sops(&args, &[], neutral.as_deref(), Some(&value))?;
            if out.code == 0 {
                Ok(())
            } else {
                Err(sops_error(&out.stderr))
            }
        };
        let restore_on_error = |result: Res<()>| {
            if result.is_err() {
                let _ = std::fs::write(file, &on_disk.bytes);
            }
            result.map(|()| true)
        };
        if !meta.format.is_env() {
            // A textarea turns Windows line endings into \n; keep the file's own.
            let text = if before.contains("\r\n") && !plaintext.contains("\r\n") {
                plaintext.replace('\n', "\r\n")
            } else {
                plaintext
            };
            if text == before {
                return Ok(true);
            }
            if meta.format.whole {
                // One value: replace it in place, which keeps every key and option.
                return restore_on_error(set("[\"data\"]", &text));
            }
            return self
                .reencrypt(file, &meta, &text, &meta.recipients)
                .map(|()| true);
        }
        // Change an env file one variable at a time. Untouched variables keep
        // their ciphertext, which keeps Git diffs small.
        let (changes, _) = env_changes(&before, &plaintext);
        if changes.is_empty() {
            return Ok(true);
        }
        let key = |change: &Value| change["k"].as_str().unwrap_or("").to_string();
        // sops addresses a variable as ['NAME']; names with these characters can't be written that way.
        if changes
            .iter()
            .any(|c| key(c).contains(['[', ']', '\'', '"']))
        {
            return self
                .reencrypt(file, &meta, &plaintext, &meta.recipients)
                .map(|()| true);
        }
        let applied = changes.iter().try_for_each(|change| {
            let index = format!("['{}']", key(change));
            if change["kind"] == "removed" {
                let mut args = vec!["unset"];
                args.extend(meta.format.store_args());
                args.extend([&*path, index.as_str()]);
                let out = self.sops(&args, &[], neutral.as_deref(), None)?;
                return if out.code == 0 {
                    Ok(())
                } else {
                    Err(sops_error(&out.stderr))
                };
            }
            set(&index, change["new"].as_str().unwrap_or(""))
        });
        restore_on_error(applied)
    }

    fn set_recipients(&self, file: &Path, recipients: &[String]) -> Res<()> {
        if recipients.is_empty() {
            return Err("A file needs at least one recipient.".into());
        }
        if recipients.iter().any(|k| !valid_public_key(k)) {
            return Err("That isn't a valid age public key.".into());
        }
        let (_, meta) = self.load(file)?;
        let plaintext = self.decrypt(file, meta.format)?;
        self.reencrypt(file, &meta, &plaintext, recipients)
    }

    // ---- views -----------------------------------------------------------

    fn describe_file(&self, file: &Listed, git: Option<GitState>) -> Value {
        let path = file.path.to_string_lossy();
        let root = repo_root(&file.path);
        let name = file
            .path
            .strip_prefix(&self.folder)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| path.to_string());
        let mut info = json!({
            "path": path,
            "name": name,
            // Fingerprint of the bytes on disk; the UI compares it with the open
            // file's to notice changes made outside the app, such as a git pull.
            "rev": file.rev,
            "repo": root.as_ref().map(|r| r.to_string_lossy().into_owned()),
            // Path inside the repository; the file list groups by it.
            "rel": root.as_ref().and_then(|r| file.path.strip_prefix(r).ok()).map(|p| p.to_string_lossy().into_owned()),
            "git": git.map(GitState::as_str),
            "format": file.meta.as_ref().map(|m| m.format.name()),
        });
        let mut set = |status: &str, note: &str| {
            info["status"] = json!(status);
            info["note"] = json!(note);
        };
        let Some(meta) = &file.meta else {
            let (status, note) = file
                .problem
                .clone()
                .unwrap_or(("unreadable", String::new()));
            set(status, &note);
            return info;
        };
        match meta.recipients.iter().find_map(|r| self.label_for(r)) {
            Some(label) => set("ok", &format!("Opens with {label}")),
            None => set("locked", "No matching key"),
        }
        info["recipients"] = json!(meta.recipients);
        info["drift"] = json!(file.drift);
        info
    }

    /// What a scan needs, to run without the app's lock (see `ScanJob`).
    pub fn scan_job(&self) -> ScanJob {
        ScanJob {
            folder: self.folder.clone(),
            depth: self.scan_depth(),
            skip: self.skip_dirs(),
            extra: self.extra_files.clone(),
            cache: self.scan_cache.clone(),
            git: self.git.clone(),
        }
    }

    /// The app's state from a finished scan. A scan of a folder that is no
    /// longer the open one is done again.
    pub fn finish_scan(&mut self, scan: ScanResult) -> Value {
        if scan.folder != self.folder {
            return self.state();
        }
        self.scan_cache = scan.cache;
        let files: Vec<Value> = scan
            .files
            .iter()
            .map(|f| self.describe_file(f, scan.states.get(&f.path).copied()))
            .collect();
        let keys: Vec<Value> = self
            .key_sources
            .iter()
            .map(|s| {
                json!({
                    "id": s.id, "kind": s.kind, "label": s.label, "path": s.path, "error": s.error,
                    "pubs": s.identities.iter().map(|i| &i.public).collect::<Vec<_>>(),
                })
            })
            .collect();
        json!({
            "folder": self.folder.to_string_lossy(),
            "sops": self.sops,
            "os": if cfg!(target_os = "macos") { "darwin" } else { std::env::consts::OS },
            "appDir": self.bundled_dirs.first().map(|d| d.to_string_lossy().into_owned()),
            "defaultKeyPath": default_key_path().to_string_lossy(),
            "files": files,
            // True when the scan stopped early because the folder is huge.
            "scanLimited": scan.limited,
            "repos": scan.repos,
            // False when git isn't installed: repositories are still shown, without status.
            "git": self.git.is_some(),
            "recent": self.config.recent.iter().filter(|f| Path::new(f).is_dir()).collect::<Vec<_>>(),
            "keys": keys,
            "book": self.config.book,
            "version": self.version,
            "settings": {
                "clipboardClearSeconds": self.clipboard_clear_seconds(),
                "checkUpdates": self.check_updates(),
                "scanDepth": self.scan_depth(),
                "skipDirs": self.skip_dirs(),
                "defaultSkipDirs": DEFAULT_SKIP_DIRS,
            },
        })
    }

    fn state(&mut self) -> Value {
        let scan = self.scan_job().run();
        self.finish_scan(scan)
    }

    fn open_file(&self, file: &Path) -> Res<Value> {
        let (on_disk, meta) = self.load(file)?;
        let plaintext = self.decrypt(file, meta.format)?;
        let recipients: Vec<Value> = meta
            .recipients
            .iter()
            .map(|k| json!({ "key": k, "name": self.name_for(k), "mine": self.label_for(k).is_some() }))
            .collect();
        let mut opened = json!({
            "format": meta.format.name(),
            "kind": if meta.format.is_env() { "env" } else { "text" },
            "recipients": recipients,
            "rev": on_disk.rev,
            // Why the file can't be encrypted again here (changing recipients,
            // saving a YAML, JSON or INI file), or null.
            "reencrypt": meta.reencrypt_problem(),
        });
        if meta.format.is_env() {
            opened["items"] = json!(parse_env(&plaintext));
        } else {
            opened["text"] = json!(plaintext);
        }
        Ok(opened)
    }

    /// Compares the file on disk with its last committed version, decrypted:
    /// by variable for env files, by line for everything else.
    fn changes(&self, file: &Path) -> Res<Value> {
        let root = repo_root(file).ok_or("This file isn't in a Git repository.")?;
        let git = self
            .git
            .as_ref()
            .ok_or("Git isn't installed, so there's no committed version to compare with.")?;
        let rel = repo_relative(&root, file).ok_or("This file isn't in a Git repository.")?;
        let shown =
            git_read(git, &root, &["show", &format!("HEAD:{rel}")]).map_err(|e| e.to_string())?;
        // A failing `git show` means the file has never been committed.
        let committed = shown.code == 0;
        let (_, meta) = self.load(file)?;
        let current = self.decrypt(file, meta.format)?;
        let before = match (committed, detect(&shown.stdout)) {
            (false, _) => String::new(),
            (true, None) => shown.stdout,
            (true, Some(old)) => self
                .decrypt_text(&shown.stdout, old.format)
                .map_err(|e| format!("The committed version couldn't be decrypted. {e}"))?,
        };
        if meta.format.is_env() {
            let (changes, unchanged) = env_changes(&before, &current);
            return Ok(
                json!({ "committed": committed, "kind": "env", "changes": changes, "unchanged": unchanged }),
            );
        }
        let (lines, changed) = text_changes(&before, &current);
        Ok(json!({ "committed": committed, "kind": "text", "lines": lines, "changed": changed }))
    }

    // ---- actions ---------------------------------------------------------

    /// Entry point for the UI: one action name plus a JSON body.
    pub fn handle(&mut self, action: &str, body: &Value) -> Res<Value> {
        let text = |key: &str| {
            body.get(key)
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string()
        };
        let file = || PathBuf::from(text("path"));
        let list = |key: &str| -> Vec<String> {
            serde_json::from_value(body.get(key).cloned().unwrap_or(Value::Null))
                .unwrap_or_default()
        };
        match action {
            "state" => Ok(self.state()),

            "setFolder" => {
                let path = file();
                if !path.is_dir() {
                    return Err("That isn't a folder.".into());
                }
                self.config.last_folder = Some(path.to_string_lossy().into_owned());
                self.folder = path;
                self.extra_files.clear();
                self.scan_cache = ScanCache::default();
                self.remember_folder();
                self.save_config()?;
                Ok(self.state())
            }

            "checkNewFile" => {
                let path = self.new_file_path(&text("dir"), &text("name"))?;
                let format = format_for_name(text("name").trim());
                Ok(json!({
                    "path": path.to_string_lossy(),
                    "hasRule": rule_recipients(&path).is_some(),
                    "format": format.name(),
                    "kind": if format.is_env() { "env" } else { "text" },
                }))
            }

            "create" => {
                let content = Content::from_body(body)?;
                let path =
                    self.create_file(&text("dir"), &text("name"), &content, &list("recipients"))?;
                Ok(
                    json!({ "state": self.state(), "file": self.open_file(&path)?, "path": path.to_string_lossy() }),
                )
            }

            "addFile" => {
                let path = file();
                if !path.is_file() {
                    return Err("That isn't a file.".into());
                }
                self.load(&path)?;
                if !self.extra_files.contains(&path) {
                    self.extra_files.push(path);
                }
                Ok(self.state())
            }

            "open" => self.open_file(&file()),

            "changes" => self.changes(&file()),

            "save" => {
                let content = Content::from_body(body)?;
                let rev = body.get("rev").and_then(Value::as_str);
                let overwrite = body
                    .get("overwrite")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                if !self.save_file(&file(), &content, rev, overwrite)? {
                    // The UI asks the user whether to overwrite, then sends the save again.
                    return Ok(json!({ "conflict": true }));
                }
                Ok(json!({ "state": self.state(), "file": self.open_file(&file())? }))
            }

            "setRecipients" => {
                self.set_recipients(&file(), &list("recipients"))?;
                Ok(json!({ "state": self.state(), "file": self.open_file(&file())? }))
            }

            "applyRule" => {
                let expected = rule_recipients(&file())
                    .filter(|r| !r.is_empty())
                    .ok_or_else(|| {
                        "No .sops.yaml rule with age recipients matches this file.".to_string()
                    })?;
                self.set_recipients(&file(), &expected)?;
                Ok(self.state())
            }

            "addKeyPath" => {
                let path = file();
                let (_, error) = read_key_file(&path);
                if let Some(e) = error {
                    return Err(format!("{e}."));
                }
                let as_text = path.to_string_lossy().into_owned();
                if path == default_key_path()
                    || self.config.key_paths.iter().any(|k| k.path == as_text)
                {
                    return Err("That key file is already loaded.".into());
                }
                let fallback = path
                    .file_stem()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "key".into());
                let label = if text("label").trim().is_empty() {
                    fallback
                } else {
                    text("label").trim().to_string()
                };
                self.config.key_paths.push(KeyPath {
                    path: as_text,
                    label,
                });
                self.save_config()?;
                self.reload_keys();
                Ok(self.state())
            }

            "addSessionKey" => {
                let identities = parse_identities(&text("content"))?;
                if identities.is_empty() {
                    return Err("No age secret key found. It starts with AGE-SECRET-KEY-.".into());
                }
                let id = self.new_session_key(&text("label"), "pasted", identities);
                self.reload_keys();
                Ok(json!({ "state": self.state(), "id": id }))
            }

            "generateKey" => {
                let fresh = age::x25519::Identity::generate();
                let identity = Identity {
                    secret: fresh.to_string().expose_secret().to_string(),
                    public: fresh.to_public().to_string(),
                };
                let public = identity.public.clone();
                if body
                    .get("persist")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                {
                    append_to_default(&[identity])?;
                } else {
                    self.new_session_key(&text("label"), "new", vec![identity]);
                }
                self.reload_keys();
                Ok(json!({ "state": self.state(), "pub": public }))
            }

            "persistKey" => {
                let id = text("id");
                let index = self
                    .session_keys
                    .iter()
                    .position(|s| s.id == id)
                    .ok_or_else(|| "That key is no longer loaded.".to_string())?;
                append_to_default(&self.session_keys[index].identities)?;
                self.session_keys.remove(index);
                self.reload_keys();
                Ok(self.state())
            }

            "removeKey" => {
                let id = text("id");
                if let Some(path) = id.strip_prefix("path:") {
                    self.config.key_paths.retain(|k| k.path != path);
                    self.save_config()?;
                } else {
                    self.session_keys.retain(|s| s.id != id);
                }
                self.reload_keys();
                Ok(self.state())
            }

            "bookAdd" => {
                let (name, key) = (
                    text("name").trim().to_string(),
                    text("key").trim().to_string(),
                );
                if name.is_empty() {
                    return Err("Enter a name.".into());
                }
                if !valid_public_key(&key) {
                    return Err("That isn't a valid age public key. It starts with age1.".into());
                }
                self.config.book.retain(|b| b.key != key);
                self.config.book.push(BookEntry { name, key });
                self.save_config()?;
                Ok(self.state())
            }

            // A key pair for a server or CI: the public key becomes a saved
            // recipient, the private key is copied once (through the shell's
            // `copy`) into that machine's secret store and then forgotten.
            "machineKeyCreate" => {
                let name = text("name").trim().to_string();
                if name.is_empty() {
                    return Err("Enter a name for the server or CI.".into());
                }
                let fresh = age::x25519::Identity::generate();
                let identity = Identity {
                    secret: fresh.to_string().expose_secret().to_string(),
                    public: fresh.to_public().to_string(),
                };
                let public = identity.public.clone();
                self.machine_key = Some((name.clone(), identity));
                Ok(json!({ "name": name, "pub": public }))
            }

            "machineKeyFinish" => {
                let (name, identity) = self
                    .machine_key
                    .take()
                    .ok_or("That key is gone. Create a new one.")?;
                self.config.book.retain(|b| b.key != identity.public);
                self.config.book.push(BookEntry {
                    name,
                    key: identity.public,
                });
                self.save_config()?;
                Ok(self.state())
            }

            "machineKeyDiscard" => {
                self.machine_key = None;
                Ok(self.state())
            }

            "setSettings" => {
                // Each setting is optional, so the UI can change one at a time.
                if let Some(seconds) = body.get("clipboardClearSeconds") {
                    let seconds = seconds
                        .as_u64()
                        .filter(|s| *s <= 3600)
                        .ok_or("Choose a time of up to one hour.")?;
                    self.config.clipboard_clear_seconds = Some(seconds as u32);
                }
                if let Some(on) = body.get("checkUpdates") {
                    self.config.check_updates = Some(on.as_bool().ok_or("Choose on or off.")?);
                }
                if let Some(depth) = body.get("scanDepth") {
                    let depth = depth
                        .as_u64()
                        .filter(|d| (1..=u64::from(MAX_SCAN_DEPTH)).contains(d))
                        .ok_or("Choose a depth from 1 to 10.")?;
                    self.config.scan_depth = Some(depth as u32);
                }
                if let Some(dirs) = body.get("skipDirs") {
                    let dirs: Vec<String> = serde_json::from_value::<Vec<String>>(dirs.clone())
                        .map_err(|_| "Enter folder names separated by commas.".to_string())?
                        .iter()
                        .map(|d| d.trim().to_string())
                        .filter(|d| !d.is_empty())
                        .collect();
                    if dirs.iter().any(|d| d.contains(['/', '\\'])) {
                        return Err("Enter folder names, not paths, such as node_modules.".into());
                    }
                    let defaults: Vec<String> =
                        DEFAULT_SKIP_DIRS.iter().map(|d| d.to_string()).collect();
                    self.config.skip_dirs = (dirs != defaults).then_some(dirs);
                }
                self.save_config()?;
                Ok(self.state())
            }

            "pickUpdate" => Ok(json!({
                "current": self.version,
                "update": pick_update(&self.version, body.get("releases").unwrap_or(&Value::Null)),
            })),

            "bookRemove" => {
                let key = text("key");
                self.config.book.retain(|b| b.key != key);
                self.save_config()?;
                Ok(self.state())
            }

            _ => Err("Unknown action.".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What sops writes for an env file, close enough for recognising it:
    /// the app finds files without needing sops or a key.
    const FAKE_ENCRYPTED: &str = "A=ENC[AES256_GCM,data:YQ==,iv:aXY=,tag:dGFn,type:str]\n\
        sops_age__list_0__map_recipient=age1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq\n\
        sops_mac=ENC[AES256_GCM,data:bWFj,iv:aXY=,tag:dGFn,type:str]\n\
        sops_version=3.13.3\n";

    #[test]
    fn env_round_trip_keeps_comments_and_equals_signs() {
        let text = "A=1\n# note\n\nB=x=y z\n";
        let items = parse_env(text);
        assert_eq!(
            items[3],
            Item::Kv {
                k: "B".into(),
                v: "x=y z".into()
            }
        );
        assert_eq!(serialize_env(&items).unwrap(), text);
    }

    #[test]
    fn serialize_rejects_bad_input() {
        let kv = |k: &str, v: &str| Item::Kv {
            k: k.into(),
            v: v.into(),
        };
        assert!(serialize_env(&[kv("A", "1"), kv("A", "2")])
            .unwrap_err()
            .contains("twice"));
        assert!(serialize_env(&[kv("1A", "1")]).is_err());
        assert!(serialize_env(&[kv("sops_x", "1")]).is_err());
        assert!(serialize_env(&[kv("A", "a\nb")]).is_err());
    }

    #[test]
    fn formats_for_new_file_names() {
        for name in [
            ".env",
            ".env.prod",
            "prod.env",
            ".env.prod.enc",
            "Secrets.ENV",
        ] {
            assert_eq!(format_for_name(name), Format::DOTENV, "{name}");
        }
        let store = |name: &str| format_for_name(name).store;
        assert_eq!(store("config.enc.yaml"), Store::Yaml);
        assert_eq!(store("x.YML"), Store::Yaml);
        assert_eq!(store("secrets.env.json"), Store::Json);
        assert_eq!(store("db.ini"), Store::Ini);
        for name in ["valkey-users.enc.acl", ".envrc", "notes", "id_rsa"] {
            assert_eq!(format_for_name(name), Format::BINARY, "{name}");
        }
    }

    /// Files are recognised by their sops metadata, whatever their name.
    #[test]
    fn recognises_sops_files_by_content() {
        let mac = "ENC[AES256_GCM,data:x,iv:y,tag:z,type:str]";
        let age = |key: &str| format!("        - recipient: {key}\n          enc: x\n");
        let yaml = format!(
            "db:\n    password: ENC[AES256_GCM,data:a,iv:b,tag:c,type:str]\nsops:\n    age:\n{}{}    mac: {mac}\n    encrypted_regex: ^password$\n    version: 3.13.3\n",
            age("age1aaa"),
            age("age1bbb")
        );
        let m = detect(&yaml).unwrap();
        assert_eq!(
            m.format,
            Format {
                store: Store::Yaml,
                whole: false
            }
        );
        assert_eq!(m.recipients, ["age1aaa", "age1bbb"]);
        assert_eq!(m.options, [("--encrypted-regex", "^password$".to_string())]);
        assert!(!m.other_keys && m.cant_keep.is_none());

        let json = format!(
            r#"{{"data": "ENC[x]", "sops": {{"age": [{{"recipient": "age1ccc"}}], "pgp": [{{"fp": "AB"}}], "mac": "{mac}", "unencrypted_suffix": "_unencrypted"}}}}"#
        );
        let m = detect(&json).unwrap();
        assert_eq!(m.format, Format::BINARY);
        assert!(m.other_keys && m.reencrypt_problem().unwrap().contains("PGP"));
        assert_eq!(
            m.options,
            [("--unencrypted-suffix", "_unencrypted".to_string())]
        );
        let json = json.replace(r#""data""#, r#""other": 1, "data""#);
        assert_eq!(detect(&json).unwrap().format.store, Store::Json);
        assert!(!detect(&json).unwrap().format.whole);

        let ini = format!("[db]\npass = ENC[x]\n\n[sops]\nage__list_0__map_recipient = age1ddd\nmac = {mac}\nmac_only_encrypted = true\n");
        let m = detect(&ini).unwrap();
        assert_eq!(
            (m.format.store, m.recipients.clone()),
            (Store::Ini, vec!["age1ddd".to_string()])
        );
        assert!(m
            .reencrypt_problem()
            .unwrap()
            .contains("mac_only_encrypted"));

        let env = format!("A=ENC[x]\nsops_age__list_0__map_recipient=age1eee\nsops_key_groups__list_0__age__list_0__map_recipient=age1fff\nsops_mac={mac}\nsops_version=3.13.3\n");
        let m = detect(&env).unwrap();
        assert_eq!(m.format, Format::DOTENV);
        assert_eq!(m.recipients, ["age1eee", "age1fff"]);
        let data = format!("data=ENC[x]\nsops_mac={mac}\n");
        assert_eq!(
            detect(&data).unwrap().format,
            Format {
                store: Store::Dotenv,
                whole: true
            }
        );

        // Mentioning sops isn't enough.
        for text in [
            "A=1\nB=2\n",
            "sops:\n  note: not encrypted\n",
            r#"{"sops": {"version": "3"}}"#,
            "[sops]\nversion = 3\n",
            "# sops_mac=nothing\n",
            "",
        ] {
            assert!(detect(text).is_none(), "{text}");
        }
    }

    #[test]
    fn text_changes_by_line() {
        let old = "a\nb\nc\nd\ne\nf\ng\nh\n";
        let new = "a\nb\nc\nd\nE\nf\ng\nh\nI\n";
        let (lines, changed) = text_changes(old, new);
        let shape: Vec<String> = lines
            .iter()
            .map(|l| match l["kind"].as_str().unwrap() {
                "skip" => format!("…{}", l["count"]),
                "added" => format!("+{}", l["text"].as_str().unwrap()),
                "removed" => format!("-{}", l["text"].as_str().unwrap()),
                _ => format!(" {}", l["text"].as_str().unwrap()),
            })
            .collect();
        assert_eq!(
            shape,
            ["…2", " c", " d", "-e", "+E", " f", " g", " h", "+I"]
        );
        assert_eq!(changed, 3);
        let (lines, changed) = text_changes("same\r\n", "same\n");
        assert!(
            lines.is_empty() && changed == 0,
            "line endings alone aren't a change"
        );
        let (lines, _) = text_changes("", "new\n");
        assert_eq!(lines, [json!({ "kind": "added", "text": "new" })]);
    }

    /// The scan stops after a fixed number of folder entries and says so.
    #[test]
    fn scan_skips_folders_and_stops_in_huge_folders() {
        let root = std::env::temp_dir().join(format!("open-sops-ui-walk-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for dir in ["a/b/c", "node_modules/x", ".hidden", "keep"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        for file in [
            "top",
            "a/one",
            "a/b/two",
            "a/b/c/three",
            "node_modules/x/dep",
            ".hidden/h",
            "keep/k",
        ] {
            std::fs::write(root.join(file), "x").unwrap();
        }
        let names = |depth: u32, skip: &[&str], budget: usize| {
            let skip: Vec<String> = skip.iter().map(|s| s.to_string()).collect();
            let (mut out, mut left) = (Vec::new(), budget);
            walk(&root, depth, &skip, &mut out, &mut left);
            let mut names: Vec<String> = out
                .iter()
                .map(|(p, _)| p.file_name().unwrap().to_string_lossy().into_owned())
                .collect();
            names.sort();
            (names, left == 0)
        };
        assert_eq!(
            names(3, &["node_modules"], 1000),
            (
                vec![
                    "k".into(),
                    "one".into(),
                    "three".into(),
                    "top".into(),
                    "two".into()
                ],
                false
            )
        );
        assert_eq!(names(1, &["node_modules"], 1000).0, ["k", "one", "top"]);
        assert!(
            names(3, &[], 1000).0.contains(&"dep".to_string()),
            "the skip list is the caller's"
        );
        assert!(
            names(3, &[], 1000).0.iter().all(|n| n != "h"),
            "hidden folders are always skipped"
        );
        assert!(
            names(3, &[], 3).1,
            "a scan that runs out of entries says so"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn sops_version_floor() {
        assert!(
            version_supported("3.11.0")
                && version_supported("3.13.3")
                && version_supported("4.0.0")
        );
        assert!(
            !version_supported("3.10.2")
                && !version_supported("2.99.0")
                && !version_supported("junk")
        );
    }

    #[test]
    fn env_changes_by_variable() {
        let (changes, unchanged) = env_changes("A=1\n# c\nB=2\nC=3\n", "A=1\nB=9\nD=4\n");
        let kinds: Vec<_> = changes
            .iter()
            .map(|c| {
                format!(
                    "{}:{}",
                    c["k"].as_str().unwrap(),
                    c["kind"].as_str().unwrap()
                )
            })
            .collect();
        assert_eq!(kinds, ["B:changed", "D:added", "C:removed"]);
        assert_eq!(unchanged, 1);
    }

    /// Git is optional: without the binary, repositories are still found and
    /// named from the file system, and everything else keeps working.
    #[test]
    fn works_without_git_installed() {
        let root = std::env::temp_dir().join(format!("open-sops-ui-nogit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let repo = root.join("shop");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::create_dir_all(repo.join("api")).unwrap();
        std::fs::write(
            repo.join(".git").join("HEAD"),
            "ref: refs/heads/feature/login\n",
        )
        .unwrap();
        std::fs::write(repo.join("api").join(".env"), FAKE_ENCRYPTED).unwrap();
        std::fs::write(repo.join("plain.env"), "B=2\n").unwrap();
        std::fs::write(root.join("loose.env"), FAKE_ENCRYPTED).unwrap();
        let file = repo.join("api").join(".env");

        assert_eq!(repo_root(&file), Some(repo.clone()));
        assert_eq!(repo_root(&root.join("loose.env")), repo_root(&root));
        assert_eq!(repo_branch(&repo).as_deref(), Some("feature/login"));
        assert_eq!(repo_relative(&repo, &file).as_deref(), Some("api/.env"));

        let mut app = App::new(Some(root.clone()), vec![]);
        app.git = None;
        let state = app.state();
        assert_eq!(state["git"], false);
        let in_repo = state["files"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["path"] == file.to_string_lossy().as_ref())
            .unwrap();
        assert_eq!(in_repo["repo"], repo.to_string_lossy().as_ref());
        assert!(in_repo["git"].is_null());
        assert_eq!(in_repo["status"], "locked");
        assert_eq!(in_repo["format"], "dotenv");
        assert!(
            state["files"]
                .as_array()
                .unwrap()
                .iter()
                .all(|f| f["name"] != "shop/plain.env"),
            "plaintext files aren't listed"
        );
        if repo_root(&root).is_none() {
            assert_eq!(state["repos"].as_array().unwrap().len(), 1);
            assert_eq!(state["repos"][0]["branch"], "feature/login");
        }
        assert!(app
            .changes(&file)
            .unwrap_err()
            .contains("Git isn't installed"));

        let _ = std::fs::remove_dir_all(&root);
    }

    /// A repository's config can name a program for git to run. Listing the
    /// files of such a repository must not run it.
    #[test]
    fn untrusted_repository_config_is_not_executed() {
        let Some(git) = find_program("git") else {
            eprintln!("git not installed: skipping");
            return;
        };
        let root = std::env::temp_dir().join(format!("open-sops-ui-trap-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let repo = root.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::write(repo.join(".env"), FAKE_ENCRYPTED).unwrap();
        let marker = root.join("EXECUTED");
        let trap = format!(
            "touch '{}'; false",
            marker.to_string_lossy().replace('\\', "/")
        );
        let raw = |args: &[&str]| {
            run(&git, args, &[], Some(&repo), None)
                .map(|o| o.code == 0)
                .unwrap_or(false)
        };
        assert!(raw(&["init", "-q"]));
        assert!(raw(&["config", "core.fsmonitor", &trap]));

        // The trap is real: a plain `git status` springs it.
        raw(&["status"]);
        if !marker.exists() {
            eprintln!("this git doesn't run core.fsmonitor here: nothing to guard against");
            let _ = std::fs::remove_dir_all(&root);
            return;
        }
        std::fs::remove_file(&marker).unwrap();

        let mut app = App::new(Some(root.clone()), vec![]);
        let state = app.state();
        let listed = state["files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["git"] == "new");
        assert!(listed, "the file's Git status is still reported");
        // Runs `git show` first; decrypting may then fail, there's no key.
        let _ = app.changes(&repo.join(".env"));
        assert!(
            !marker.exists(),
            "the repository's fsmonitor program was executed"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn decodes_the_encodings_env_files_come_in() {
        let utf16 = |text: &str, le: bool, bom: bool| -> Vec<u8> {
            let mut out = if !bom {
                vec![]
            } else if le {
                vec![0xFF, 0xFE]
            } else {
                vec![0xFE, 0xFF]
            };
            for unit in text.encode_utf16() {
                out.extend(if le {
                    unit.to_le_bytes()
                } else {
                    unit.to_be_bytes()
                });
            }
            out
        };
        let text = "A=1\nNAME=Zo\u{eb}\n";
        assert_eq!(
            decode_text(text.as_bytes()),
            Some((text.to_string(), "UTF-8"))
        );
        assert_eq!(
            decode_text(&[&[0xEF, 0xBB, 0xBF][..], text.as_bytes()].concat()),
            Some((text.to_string(), "UTF-8 with BOM"))
        );
        for (le, bom) in [(true, true), (false, true), (true, false), (false, false)] {
            assert_eq!(
                decode_text(&utf16(text, le, bom)),
                Some((text.to_string(), "UTF-16")),
                "le={le} bom={bom}"
            );
        }
        assert_eq!(
            decode_text(b"N=caf\xe9 \x80\n"),
            Some(("N=caf\u{e9} \u{20ac}\n".to_string(), "Windows-1252"))
        );
        assert_eq!(
            decode_text(b"\x7fELF\x02\x01\x01\x00\x00\x00garbage\xff"),
            None
        );
        // A byte order mark must not end up in the first variable's name.
        assert_eq!(
            parse_env(&decode_text(b"\xef\xbb\xbfA=1\n").unwrap().0)[0],
            Item::Kv {
                k: "A".into(),
                v: "1".into()
            }
        );
    }

    #[test]
    fn picks_the_update_to_announce() {
        let r = |tag: &str, pre: bool, draft: bool| json!({ "tag_name": tag, "prerelease": pre, "draft": draft, "body": format!("notes {tag}") });
        let feed = json!([
            r("v1.0.0", false, false),
            r("v1.1.0-beta.1", true, false),
            r("v0.1.0-beta.5", true, false),
            r("v0.1.0-beta.4", true, false),
            r("v0.1.0", false, false),
            r("v9.9.9", false, true),       // draft
            r("nightly", true, false),      // not a version
            r("v2.0.0-rc.1", false, false), // flagged wrong on GitHub, but its tag says pre-release
        ]);
        let picked = |current: &str| {
            pick_update(current, &feed).map(|u| u["tag"].as_str().unwrap().to_string())
        };
        // A pre-release hears about the newest release of any kind.
        assert_eq!(picked("0.1.0-beta.4").as_deref(), Some("v2.0.0-rc.1"));
        let only_betas = json!([
            r("v0.1.0-beta.5", true, false),
            r("v0.1.0-beta.4", true, false)
        ]);
        assert_eq!(
            pick_update("0.1.0-beta.4", &only_betas).unwrap()["tag"],
            "v0.1.0-beta.5"
        );
        let beta_then_stable = json!([r("v0.1.0", false, false), r("v0.1.0-beta.5", true, false)]);
        assert_eq!(
            pick_update("0.1.0-beta.4", &beta_then_stable).unwrap()["tag"],
            "v0.1.0"
        );
        // A stable version never hears about a pre-release.
        assert_eq!(picked("0.1.0").as_deref(), Some("v1.0.0"));
        assert_eq!(picked("1.0.0"), None);
        // Nothing newer, nothing to say; dev builds count as pre-releases of their base.
        assert_eq!(pick_update("0.1.0-beta.5", &only_betas), None);
        assert_eq!(
            pick_update("0.1.0-dev.57", &beta_then_stable).unwrap()["tag"],
            "v0.1.0"
        );
        assert_eq!(pick_update("not a version", &feed), None);
        assert_eq!(
            pick_update("0.1.0", &json!({ "message": "rate limited" })),
            None
        );
        let u = pick_update("0.1.0", &feed).unwrap();
        assert_eq!(
            (
                u["version"].as_str(),
                u["prerelease"].as_bool(),
                u["notes"].as_str()
            ),
            (Some("1.0.0"), Some(false), Some("notes v1.0.0"))
        );
    }

    #[test]
    fn release_tags_for_urls() {
        assert!(valid_release_tag("v1.2.3") && valid_release_tag("v0.1.0-beta.4"));
        for bad in ["1.2.3", "v1.2", "v1.2.3/../../evil", "v1.2.3?x=1", "", "v"] {
            assert!(!valid_release_tag(bad), "{bad}");
        }
    }

    #[test]
    fn timestamp_shape() {
        let t = utc_timestamp();
        assert_eq!(t.len(), 20);
        assert!(t.starts_with("20") && t.ends_with('Z'));
    }
}
