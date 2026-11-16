//! Backend logic for open-sops-ui. No GUI code lives here, so it can be tested
//! on its own against a real `sops` binary.
//!
//! The UI talks to [`App::handle`] with an action name and a JSON body.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use age::secrecy::ExposeSecret;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const DOTENV: [&str; 4] = ["--input-type", "dotenv", "--output-type", "dotenv"];
const EDIT_ADDR: &str = "OPEN_SOPS_UI_EDIT_ADDR";
const EDIT_TOKEN: &str = "OPEN_SOPS_UI_EDIT_TOKEN";

type Res<T> = Result<T, String>;

// ---------------------------------------------------------------------------
// Editor-helper mode
// ---------------------------------------------------------------------------

/// `sops edit` launches the app as its "editor". In that mode we fetch the new
/// plaintext from the running app over loopback and write it into the temp
/// file sops gave us, which keeps unchanged values byte-identical in Git.
///
/// Returns the process exit code when the process was started as an editor,
/// `None` when it should start normally.
pub fn run_as_editor_if_requested() -> Option<i32> {
    let addr = std::env::var(EDIT_ADDR).ok()?;
    let token = std::env::var(EDIT_TOKEN).ok()?;
    let target = std::env::args().last()?;
    let fetch = || -> std::io::Result<()> {
        let mut stream = TcpStream::connect(addr)?;
        stream.write_all(token.as_bytes())?;
        stream.write_all(b"\n")?;
        let mut text = Vec::new();
        stream.read_to_end(&mut text)?;
        if text.is_empty() {
            return Err(std::io::Error::other("no content"));
        }
        std::fs::write(&target, text)
    };
    Some(if fetch().is_ok() { 0 } else { 1 })
}

/// Serves `plaintext` once on a loopback port to a client that sends `token`.
struct EditHandoff {
    addr: String,
    token: String,
    done: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl EditHandoff {
    fn start(plaintext: String) -> Res<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let addr = listener
            .local_addr()
            .map_err(|e| e.to_string())?
            .to_string();
        let token = random_token();
        let done = Arc::new(AtomicBool::new(false));
        let (flag, expected) = (done.clone(), token.clone());
        let thread = std::thread::spawn(move || {
            while !flag.load(Ordering::Relaxed) {
                let Ok((mut stream, _)) = listener.accept() else {
                    std::thread::sleep(Duration::from_millis(15));
                    continue;
                };
                let _ = stream.set_nonblocking(false);
                let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                let mut got = Vec::new();
                let mut byte = [0u8; 1];
                while got.len() < 128 && matches!(stream.read(&mut byte), Ok(1)) && byte[0] != b'\n'
                {
                    got.push(byte[0]);
                }
                if got == expected.as_bytes() {
                    let _ = stream.write_all(plaintext.as_bytes());
                    return;
                }
            }
        });
        Ok(Self {
            addr,
            token,
            done,
            thread: Some(thread),
        })
    }
}

impl Drop for EditHandoff {
    fn drop(&mut self) {
        self.done.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn random_token() -> String {
    // A fresh age identity is 32 random bytes from the OS; reuse it as a token.
    let id = age::x25519::Identity::generate();
    id.to_public().to_string()
}

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
}

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
    match std::fs::read_to_string(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            (vec![], Some("No file found here".into()))
        }
        Err(e) => (vec![], Some(e.to_string())),
        Ok(text) => match parse_identities(&text) {
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
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
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
    let mut text = existing;
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    for i in fresh {
        text.push_str(&format!(
            "# created: {}\n# public key: {}\n{}\n",
            utc_timestamp(),
            i.public,
            i.secret
        ));
    }
    write_private(&path, &text)
}

fn write_private(path: &Path, text: &str) -> Res<()> {
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut file = opts.open(path).map_err(io_msg)?;
    file.write_all(text.as_bytes()).map_err(io_msg)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).map_err(io_msg)?;
    }
    Ok(())
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

/// Oldest sops release the end-to-end test passes with; CI tests against it.
pub const MIN_SOPS_VERSION: (u32, u32) = (3, 10);

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
        .filter(|l| !l.is_empty())
        .last()
        .unwrap_or("sops failed.")
        .to_string()
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

fn file_recipients(text: &str) -> Vec<String> {
    let re = Regex::new(r"(?m)^sops_age__list_\d+__map_recipient=(age1[a-z0-9]+)\s*$").unwrap();
    re.captures_iter(text).map(|c| c[1].to_string()).collect()
}

fn is_encrypted(text: &str) -> bool {
    text.lines()
        .any(|l| l.starts_with("sops_version=") || l.starts_with("sops_mac="))
}

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
    fn tracked(self) -> bool {
        matches!(self, GitState::Clean | GitState::Modified)
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
        let out = run(git, &all, &[], Some(root), None).ok()?;
        (out.code == 0).then_some(out.stdout)
    };
    let tracked: BTreeSet<String> = call(&["ls-files", "-z"])?
        .split('\0')
        .filter(|p| !p.is_empty())
        .map(str::to_string)
        .collect();
    // Entries look like "XY path"; a rename is followed by its old path.
    let mut changed: BTreeMap<String, GitState> = BTreeMap::new();
    let status = call(&["status", "--porcelain=v1", "-z", "--untracked-files=all"])?;
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

/// Committed on purpose with placeholder values, like `.env.example`.
fn is_template(name: &str) -> bool {
    let lower = name.to_lowercase();
    ["example", "sample", "template", "dist"]
        .iter()
        .any(|word| lower.split('.').any(|part| part == *word))
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

// ---------------------------------------------------------------------------
// The app
// ---------------------------------------------------------------------------

const SKIP_DIRS: [&str; 8] = [
    "node_modules",
    ".git",
    "vendor",
    "dist",
    "build",
    "target",
    ".venv",
    "__pycache__",
];

fn looks_like_env(name: &str) -> bool {
    name != ".sops.yaml" && name.to_lowercase().split('.').any(|part| part == "env")
}

fn scan(dir: &Path, depth: u32, out: &mut Vec<PathBuf>) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = read.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        if out.len() >= 200 {
            return;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        if path.is_file() && looks_like_env(&name) {
            out.push(path);
        } else if path.is_dir()
            && depth > 0
            && !name.starts_with('.')
            && !SKIP_DIRS.contains(&name.as_str())
        {
            scan(&path, depth - 1, out);
        }
    }
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
    editor_exe: PathBuf,
    next_id: u32,
}

impl App {
    /// `bundled_dirs` are searched for a shipped sops binary before PATH.
    /// `editor_exe` is the binary sops launches as its editor when saving; it
    /// must call [`run_as_editor_if_requested`] first thing in `main`.
    pub fn new(folder: Option<PathBuf>, bundled_dirs: Vec<PathBuf>, editor_exe: PathBuf) -> Self {
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
            editor_exe,
            next_id: 0,
        };
        app.remember_folder();
        app.reload_keys();
        app
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

    fn decrypt(&self, file: &Path) -> Res<String> {
        let path = file.to_string_lossy();
        let mut args = vec!["decrypt"];
        args.extend(DOTENV);
        args.push(&path);
        let out = self.sops(&args, &[], neutral_dir().as_deref(), None)?;
        if out.code != 0 {
            return Err(sops_error(&out.stderr));
        }
        Ok(out.stdout)
    }

    /// Decrypts encrypted content that isn't on disk, such as a committed version.
    fn decrypt_text(&self, encrypted: &str) -> Res<String> {
        let mut args = vec!["decrypt"];
        args.extend(DOTENV);
        args.extend(["--filename-override", "committed.env"]);
        let out = self.sops(&args, &[], neutral_dir().as_deref(), Some(encrypted))?;
        if out.code != 0 {
            return Err(sops_error(&out.stderr));
        }
        Ok(out.stdout)
    }

    /// Encrypts plaintext from stdin and writes the result to `file`.
    /// With `recipients == None` the matching .sops.yaml rule decides.
    fn encrypt_to(&self, file: &Path, plaintext: &str, recipients: Option<&[String]>) -> Res<()> {
        let path = file.to_string_lossy();
        let joined = recipients.map(|r| r.join(","));
        let mut args = vec!["encrypt"];
        if let Some(list) = &joined {
            args.extend(["--age", list.as_str()]);
        }
        args.extend(DOTENV);
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
        std::fs::write(file, out.stdout).map_err(io_msg)
    }

    fn save_file(&self, file: &Path, items: &[Item], recipients: &[String]) -> Res<()> {
        let plaintext = serialize_env(items)?;
        let current = std::fs::read_to_string(file).unwrap_or_default();
        if !is_encrypted(&current) {
            return match rule_recipients(file) {
                Some(_) => self.encrypt_to(file, &plaintext, None),
                None if recipients.is_empty() => {
                    Err("Choose at least one recipient to encrypt for.".into())
                }
                None => self.encrypt_to(file, &plaintext, Some(recipients)),
            };
        }
        let handoff = EditHandoff::start(plaintext)?;
        // sops splits the editor command like a shell would, so quote the path
        // and use forward slashes, which Windows accepts too.
        let editor = format!(
            "\"{}\"",
            self.editor_exe.to_string_lossy().replace('\\', "/")
        );
        let env = [
            ("SOPS_EDITOR", editor.clone()),
            ("EDITOR", editor),
            (EDIT_ADDR, handoff.addr.clone()),
            (EDIT_TOKEN, handoff.token.clone()),
        ];
        let path = file.to_string_lossy();
        let mut args = vec!["edit"];
        args.extend(DOTENV);
        args.push(&path);
        let out = self.sops(&args, &env, neutral_dir().as_deref(), None)?;
        // 200 means "file has not changed", which is fine.
        if out.code != 0 && out.code != 200 {
            return Err(sops_error(&out.stderr));
        }
        Ok(())
    }

    fn set_recipients(&self, file: &Path, recipients: &[String]) -> Res<()> {
        if recipients.is_empty() {
            return Err("A file needs at least one recipient.".into());
        }
        if recipients.iter().any(|k| !valid_public_key(k)) {
            return Err("That isn't a valid age public key.".into());
        }
        let plaintext = self.decrypt(file)?;
        self.encrypt_to(file, &plaintext, Some(recipients))
    }

    // ---- views -----------------------------------------------------------

    fn describe_file(&self, file: &Path, root: Option<&Path>, git: Option<GitState>) -> Value {
        let path = file.to_string_lossy();
        let name = file
            .strip_prefix(&self.folder)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| path.to_string());
        let mut info = json!({
            "path": path,
            "name": name,
            "repo": root.map(|r| r.to_string_lossy().into_owned()),
            // Path inside the repository; the file list groups by it.
            "rel": root.and_then(|r| file.strip_prefix(r).ok()).map(|p| p.to_string_lossy().into_owned()),
            "git": git.map(GitState::as_str),
        });
        let mut set = |status: &str, note: &str| {
            info["status"] = json!(status);
            info["note"] = json!(note);
        };
        let Ok(text) = std::fs::read_to_string(file) else {
            set("missing", "File not found");
            return info;
        };
        if !is_encrypted(&text) {
            let base = file
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            match git {
                _ if is_template(&base) => set("plain", "Example file, plaintext by design"),
                Some(state) if state.tracked() => {
                    set("plain-tracked", "Plaintext and committed to Git")
                }
                Some(GitState::New) => set("plain-risk", "Plaintext and not ignored by Git"),
                Some(GitState::Ignored) => set("plain", "Plaintext, ignored by Git"),
                _ => set("plain", "Plaintext"),
            }
            return info;
        }
        let recipients = file_recipients(&text);
        let drift = rule_recipients(file).and_then(|expected| {
            let missing: Vec<_> = expected
                .iter()
                .filter(|k| !recipients.contains(k))
                .cloned()
                .collect();
            let extra: Vec<_> = recipients
                .iter()
                .filter(|k| !expected.contains(k))
                .cloned()
                .collect();
            (!missing.is_empty() || !extra.is_empty())
                .then(|| json!({ "missing": missing, "extra": extra }))
        });
        match recipients.iter().find_map(|r| self.label_for(r)) {
            Some(label) => set("ok", &format!("Opens with {label}")),
            None => set("locked", "No matching key"),
        }
        info["recipients"] = json!(recipients);
        info["drift"] = json!(drift);
        info
    }

    fn state(&self) -> Value {
        let mut found = Vec::new();
        scan(&self.folder, 3, &mut found);
        for extra in &self.extra_files {
            if !found.contains(extra) {
                found.push(extra.clone());
            }
        }
        // Group the files by repository, then ask git once per repository.
        let mut by_repo: BTreeMap<PathBuf, Vec<PathBuf>> = BTreeMap::new();
        for file in &found {
            if let Some(root) = repo_root(file) {
                by_repo.entry(root).or_default().push(file.clone());
            }
        }
        let mut states: BTreeMap<PathBuf, GitState> = BTreeMap::new();
        let mut repos = Vec::new();
        for (root, files) in &by_repo {
            if let Some(found) = self
                .git
                .as_ref()
                .and_then(|git| git_states(git, root, files))
            {
                states.extend(found);
            }
            repos.push(json!({
                "root": root.to_string_lossy(),
                "name": root.file_name().map(|n| n.to_string_lossy().into_owned()),
                "branch": repo_branch(root),
            }));
        }
        let files: Vec<Value> = found
            .iter()
            .map(|f| self.describe_file(f, repo_root(f).as_deref(), states.get(f).copied()))
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
            "repos": repos,
            // False when git isn't installed: repositories are still shown, without status.
            "git": self.git.is_some(),
            "recent": self.config.recent.iter().filter(|f| Path::new(f).is_dir()).collect::<Vec<_>>(),
            "keys": keys,
            "book": self.config.book,
        })
    }

    fn open_file(&self, file: &Path) -> Res<Value> {
        let text = std::fs::read_to_string(file).map_err(io_msg)?;
        if !is_encrypted(&text) {
            return Ok(json!({
                "encrypted": false, "items": parse_env(&text), "recipients": [],
                "hasRule": rule_recipients(file).is_some(),
            }));
        }
        let plaintext = self.decrypt(file)?;
        let recipients: Vec<Value> = file_recipients(&text)
            .iter()
            .map(|k| json!({ "key": k, "name": self.name_for(k), "mine": self.label_for(k).is_some() }))
            .collect();
        Ok(
            json!({ "encrypted": true, "items": parse_env(&plaintext), "recipients": recipients, "hasRule": true }),
        )
    }

    /// Compares the file on disk with its last committed version, decrypted.
    fn changes(&self, file: &Path) -> Res<Value> {
        let root = repo_root(file).ok_or("This file isn't in a Git repository.")?;
        let git = self
            .git
            .as_ref()
            .ok_or("Git isn't installed, so there's no committed version to compare with.")?;
        let rel = repo_relative(&root, file).ok_or("This file isn't in a Git repository.")?;
        let on_disk = std::fs::read_to_string(file).map_err(io_msg)?;
        let current = if is_encrypted(&on_disk) {
            self.decrypt(file)?
        } else {
            on_disk
        };
        let shown = run(
            git,
            &["show", &format!("HEAD:{rel}")],
            &[],
            Some(&root),
            None,
        )
        .map_err(|e| e.to_string())?;
        // A failing `git show` means the file has never been committed.
        let committed = shown.code == 0;
        let before = match (committed, is_encrypted(&shown.stdout)) {
            (false, _) => String::new(),
            (true, false) => shown.stdout,
            (true, true) => self
                .decrypt_text(&shown.stdout)
                .map_err(|e| format!("The committed version couldn't be decrypted. {e}"))?,
        };
        let (changes, unchanged) = env_changes(&before, &current);
        Ok(json!({ "committed": committed, "changes": changes, "unchanged": unchanged }))
    }

    /// Adds the file to its repository's .gitignore. Needs no git binary.
    fn ignore(&self, file: &Path) -> Res<()> {
        let root = repo_root(file).ok_or("This file isn't in a Git repository.")?;
        let rel = repo_relative(&root, file).ok_or("This file isn't in a Git repository.")?;
        let path = root.join(".gitignore");
        let mut text = std::fs::read_to_string(&path).unwrap_or_default();
        let line = format!("/{rel}");
        if text.lines().any(|l| l.trim() == line) {
            return Ok(());
        }
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(&line);
        text.push('\n');
        std::fs::write(path, text).map_err(io_msg)
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
                self.remember_folder();
                self.save_config()?;
                Ok(self.state())
            }

            "addFile" => {
                let path = file();
                if !path.is_file() {
                    return Err("That isn't a file.".into());
                }
                if !self.extra_files.contains(&path) {
                    self.extra_files.push(path);
                }
                Ok(self.state())
            }

            "open" => self.open_file(&file()),

            "changes" => self.changes(&file()),

            "ignore" => {
                self.ignore(&file())?;
                Ok(self.state())
            }

            "save" => {
                let items: Vec<Item> =
                    serde_json::from_value(body.get("items").cloned().unwrap_or(Value::Null))
                        .map_err(|_| "The file content couldn't be read.".to_string())?;
                self.save_file(&file(), &items, &list("recipients"))?;
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
    fn env_file_names() {
        for name in [
            ".env",
            ".env.prod",
            "prod.env",
            ".env.prod.enc",
            "Secrets.ENV",
        ] {
            assert!(looks_like_env(name), "{name}");
        }
        for name in [".envrc", "environment.txt", ".sops.yaml"] {
            assert!(!looks_like_env(name), "{name}");
        }
    }

    #[test]
    fn sops_version_floor() {
        assert!(
            version_supported("3.10.0")
                && version_supported("3.13.3")
                && version_supported("4.0.0")
        );
        assert!(
            !version_supported("3.9.4")
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

    #[test]
    fn template_names() {
        for name in [
            ".env.example",
            "sample.env",
            ".env.template",
            ".env.dist",
            ".ENV.Example",
        ] {
            assert!(is_template(name), "{name}");
        }
        for name in [".env", ".env.production", "examples.env"] {
            assert!(!is_template(name), "{name}");
        }
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
        std::fs::write(repo.join("api").join(".env"), "A=1\n").unwrap();
        std::fs::write(root.join("loose.env"), "B=2\n").unwrap();
        let file = repo.join("api").join(".env");

        assert_eq!(repo_root(&file), Some(repo.clone()));
        assert_eq!(repo_root(&root.join("loose.env")), repo_root(&root));
        assert_eq!(repo_branch(&repo).as_deref(), Some("feature/login"));
        assert_eq!(repo_relative(&repo, &file).as_deref(), Some("api/.env"));

        let mut app = App::new(Some(root.clone()), vec![], PathBuf::from("unused"));
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
        assert_eq!(in_repo["status"], "plain");
        if repo_root(&root).is_none() {
            assert_eq!(state["repos"].as_array().unwrap().len(), 1);
            assert_eq!(state["repos"][0]["branch"], "feature/login");
        }
        assert!(app
            .changes(&file)
            .unwrap_err()
            .contains("Git isn't installed"));
        // Opening and ignoring need no git either.
        assert_eq!(app.open_file(&file).unwrap()["items"][0]["k"], "A");
        app.ignore(&file).unwrap();
        assert_eq!(
            std::fs::read_to_string(repo.join(".gitignore")).unwrap(),
            "/api/.env\n"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn timestamp_shape() {
        let t = utc_timestamp();
        assert_eq!(t.len(), 20);
        assert!(t.starts_with("20") && t.ends_with('Z'));
    }
}
