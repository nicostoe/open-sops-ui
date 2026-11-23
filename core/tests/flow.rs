//! End-to-end test against a real `sops` binary with throwaway keys.
//! Everything runs inside a temp folder; the user's own keys.txt is never read.
use open_sops_ui_core::App;
use serde_json::{json, Value};

fn pubs(state: &Value, kind: &str) -> Vec<String> {
    state["keys"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|k| k["kind"] == kind)
        .flat_map(|k| k["pubs"].as_array().unwrap().clone())
        .map(|p| p.as_str().unwrap().to_string())
        .collect()
}

fn status(state: &Value, name: &str) -> String {
    state["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == name)
        .map(|f| f["status"].as_str().unwrap().to_string())
        .unwrap_or_else(|| "absent".into())
}

#[test]
fn full_flow() {
    let root = std::env::temp_dir().join(format!("open-sops-ui-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let demo = root.join("demo");
    std::fs::create_dir_all(&demo).unwrap();
    std::env::set_var("XDG_CONFIG_HOME", root.join("xdg"));
    std::env::set_var("AppData", root.join("xdg")); // the config base on Windows
                                                    // Older sops versions fail outright when this points at a missing file.
    std::env::remove_var("SOPS_AGE_KEY_FILE");
    std::env::remove_var("SOPS_AGE_KEY");

    // Saving must never go through an editor, which would mean a temp file
    // with decrypted content. With these set, a `sops edit` would fail.
    std::env::set_var("SOPS_EDITOR", root.join("no-such-editor"));
    std::env::set_var("EDITOR", root.join("no-such-editor"));

    let mut app = App::new(Some(demo.clone()), vec![]);
    let state = app.handle("state", &json!({})).unwrap();
    if state["sops"]["path"].is_null() {
        eprintln!("sops not installed: skipping the end-to-end test");
        return;
    }
    assert!(pubs(&state, "default").is_empty());
    assert_eq!(state["keys"][0]["error"], "No file found here");

    // Create key A in the default location and key B for the session only.
    let a = app
        .handle("generateKey", &json!({ "persist": true }))
        .unwrap()["pub"]
        .as_str()
        .unwrap()
        .to_string();
    let b = app
        .handle("generateKey", &json!({ "label": "homelab" }))
        .unwrap()["pub"]
        .as_str()
        .unwrap()
        .to_string();
    let keys_txt = root.join("xdg/sops/age/keys.txt");
    assert_eq!(
        std::fs::read_to_string(&keys_txt)
            .unwrap()
            .matches("AGE-SECRET-KEY-")
            .count(),
        1
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&keys_txt).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    // A plaintext file isn't listed and isn't opened; files are created encrypted.
    std::fs::write(demo.join("plain.env"), "SECRET=visible\n").unwrap();
    assert_eq!(
        status(&app.handle("state", &json!({})).unwrap(), "plain.env"),
        "absent"
    );
    let err = app
        .handle(
            "addFile",
            &json!({ "path": demo.join("plain.env").to_string_lossy() }),
        )
        .unwrap_err();
    assert!(err.contains("isn't encrypted with SOPS"), "{err}");
    let file = demo.join(".env.prod.enc");
    let p = file.to_string_lossy().to_string();
    let items = json!([
        { "t": "kv", "k": "DB", "v": "postgres://u:p@h/db" },
        { "t": "raw", "text": "# payment" },
        { "t": "kv", "k": "STRIPE", "v": "sk_1" },
        { "t": "kv", "k": "JWT", "v": "a=b" },
    ]);
    let new_file = |app: &mut App, name: &str, items: &Value, recipients: Value| {
        app.handle(
            "create",
            &json!({ "dir": demo.to_string_lossy(), "name": name, "items": items, "recipients": recipients }),
        )
    };
    let err = new_file(&mut app, ".env.prod.enc", &items, json!([])).unwrap_err();
    assert!(err.contains("recipient"), "{err}");
    let saved = new_file(&mut app, ".env.prod.enc", &items, json!([a])).unwrap();
    assert_eq!(saved["file"]["kind"], "env");
    assert_eq!(saved["file"]["format"], "dotenv");
    assert_eq!(saved["file"]["items"], items);
    assert_eq!(saved["file"]["recipients"][0]["name"], "You (default)");
    assert_eq!(status(&saved["state"], ".env.prod.enc"), "ok");
    let before = std::fs::read_to_string(&file).unwrap();
    assert!(!before.contains("sk_1"));

    // Edit one value and add one: untouched lines keep their ciphertext.
    let mut edited = items.as_array().unwrap().clone();
    edited[2] = json!({ "t": "kv", "k": "STRIPE", "v": "sk_2" });
    edited.push(json!({ "t": "kv", "k": "NEW", "v": "hello world" }));
    let saved = app
        .handle("save", &json!({ "path": p, "items": edited }))
        .unwrap();
    assert_eq!(saved["file"]["items"], json!(edited));
    let after = std::fs::read_to_string(&file).unwrap();
    let line = |text: &str, key: &str| {
        text.lines()
            .find(|l| l.starts_with(key))
            .unwrap()
            .to_string()
    };
    assert_eq!(line(&before, "DB="), line(&after, "DB="));
    assert_eq!(line(&before, "JWT="), line(&after, "JWT="));
    assert_ne!(line(&before, "STRIPE="), line(&after, "STRIPE="));

    // Saving without changes is fine; invalid input is rejected.
    app.handle("save", &json!({ "path": p, "items": edited }))
        .unwrap();
    let dup = json!([{ "t": "kv", "k": "A", "v": "1" }, { "t": "kv", "k": "A", "v": "2" }]);
    assert!(app
        .handle("save", &json!({ "path": p, "items": dup }))
        .unwrap_err()
        .contains("twice"));

    // Recipients: add B, then check the .sops.yaml drift warning and fix.
    let both = app
        .handle("setRecipients", &json!({ "path": p, "recipients": [a, b] }))
        .unwrap();
    assert_eq!(both["file"]["recipients"].as_array().unwrap().len(), 2);
    assert!(app
        .handle(
            "setRecipients",
            &json!({ "path": p, "recipients": ["nope"] })
        )
        .is_err());
    std::fs::write(
        demo.join(".sops.yaml"),
        format!("creation_rules:\n  - path_regex: \\.env\\.prod.*\n    age: {a}\n"),
    )
    .unwrap();
    let state = app.handle("state", &json!({})).unwrap();
    let f = state["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == ".env.prod.enc")
        .unwrap();
    assert_eq!(f["drift"]["extra"][0], b);
    let state = app.handle("applyRule", &json!({ "path": p })).unwrap();
    let f = state["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == ".env.prod.enc")
        .unwrap();
    assert!(f["drift"].is_null());
    assert_eq!(f["recipients"].as_array().unwrap().len(), 1);

    // A new file matched by a rule encrypts without choosing recipients.
    let ruled = new_file(
        &mut app,
        ".env.prod.new",
        &json!([{ "t": "kv", "k": "Z", "v": "1" }]),
        json!([]),
    )
    .unwrap();
    assert_eq!(ruled["file"]["recipients"][0]["key"], a.as_str());

    // A file only B can open becomes locked once B is removed.
    let infra = demo.join("infra.env");
    let ip = infra.to_string_lossy().to_string();
    new_file(
        &mut app,
        "infra.env",
        &json!([{ "t": "kv", "k": "X", "v": "9" }]),
        json!([b]),
    )
    .unwrap();
    let state = app.handle("state", &json!({})).unwrap();
    assert_eq!(status(&state, "infra.env"), "ok");
    let session_id = state["keys"]
        .as_array()
        .unwrap()
        .iter()
        .find(|k| k["kind"] == "session")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Persist B: it moves into keys.txt next to A, and survives a restart.
    let state = app
        .handle("persistKey", &json!({ "id": session_id }))
        .unwrap();
    assert_eq!(pubs(&state, "default"), vec![a.clone(), b.clone()]);
    assert!(pubs(&state, "session").is_empty());
    assert_eq!(
        std::fs::read_to_string(&keys_txt)
            .unwrap()
            .matches("AGE-SECRET-KEY-")
            .count(),
        2
    );

    // Saved recipients are validated and persisted.
    assert!(app
        .handle("bookAdd", &json!({ "name": "x", "key": "nope" }))
        .is_err());
    app.handle("bookAdd", &json!({ "name": "Anna", "key": b }))
        .unwrap();

    // A key for a server or CI: only the public key reaches the page and the
    // config; the private key stays in memory until it is copied.
    assert!(app
        .handle("machineKeyCreate", &json!({ "name": " " }))
        .is_err());
    let made = app
        .handle("machineKeyCreate", &json!({ "name": "CI deploy" }))
        .unwrap();
    let ci = made["pub"].as_str().unwrap().to_string();
    let secret = app.machine_key_secret().unwrap();
    assert!(secret.starts_with("AGE-SECRET-KEY-1"));
    assert!(!made.to_string().contains("AGE-SECRET-KEY"));
    let parsed: age::x25519::Identity = secret.parse().unwrap();
    assert_eq!(parsed.to_public().to_string(), ci);
    let state = app.handle("machineKeyFinish", &json!({})).unwrap();
    assert!(app.machine_key_secret().is_none());
    assert!(!state.to_string().contains("AGE-SECRET-KEY"));
    assert_eq!(state["book"][1]["name"], "CI deploy");
    assert_eq!(state["book"][1]["key"], ci.as_str());
    let config_dir = root.join("xdg/open-sops-ui");
    for entry in std::fs::read_dir(&config_dir).unwrap() {
        let text = std::fs::read_to_string(entry.unwrap().path()).unwrap_or_default();
        assert!(
            !text.contains("AGE-SECRET-KEY"),
            "a private key reached the config"
        );
    }
    assert!(app.handle("machineKeyFinish", &json!({})).is_err());
    // A discarded key leaves no recipient behind.
    app.handle("machineKeyCreate", &json!({ "name": "staging" }))
        .unwrap();
    let state = app.handle("machineKeyDiscard", &json!({})).unwrap();
    assert!(app.machine_key_secret().is_none());
    assert_eq!(state["book"].as_array().unwrap().len(), 2);
    // The server decrypts a file encrypted for it with exactly the copied key.
    let ci_file = app
        .handle(
            "create",
            &json!({ "dir": demo.to_string_lossy(), "name": "ci.env",
                     "items": [{ "t": "kv", "k": "DEPLOY", "v": "from-ci" }], "recipients": [a, ci] }),
        )
        .unwrap()["path"]
        .as_str()
        .unwrap()
        .to_string();
    let elsewhere = root.join("ci-machine");
    std::fs::create_dir_all(&elsewhere).unwrap();
    let out = std::process::Command::new(state["sops"]["path"].as_str().unwrap())
        .args([
            "decrypt",
            "--input-type",
            "dotenv",
            "--output-type",
            "dotenv",
            &ci_file,
        ])
        .current_dir(&elsewhere)
        .env("XDG_CONFIG_HOME", &elsewhere)
        .env("AppData", &elsewhere)
        .env("HOME", &elsewhere)
        .env("SOPS_AGE_KEY", &secret)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "DEPLOY=from-ci\n");
    std::fs::remove_file(&ci_file).unwrap();

    // Fresh start with keys.txt removed: files are locked, opening explains why.
    std::fs::rename(&keys_txt, root.join("moved-keys.txt")).unwrap();
    let mut app = App::new(Some(demo.clone()), vec![]);
    let state = app.handle("state", &json!({})).unwrap();
    assert_eq!(state["book"][0]["name"], "Anna");
    assert_eq!(status(&state, "infra.env"), "locked");
    assert!(app
        .handle("open", &json!({ "path": ip }))
        .unwrap_err()
        .contains("None of your loaded keys"));

    // A custom key path and a pasted key both unlock it again.
    let moved = root.join("moved-keys.txt").to_string_lossy().to_string();
    let state = app
        .handle("addKeyPath", &json!({ "path": moved, "label": "work" }))
        .unwrap();
    assert_eq!(status(&state, "infra.env"), "ok");
    let state = app
        .handle("removeKey", &json!({ "id": format!("path:{moved}") }))
        .unwrap();
    assert_eq!(status(&state, "infra.env"), "locked");
    let content = std::fs::read_to_string(root.join("moved-keys.txt")).unwrap();
    let added = app
        .handle(
            "addSessionKey",
            &json!({ "content": content, "label": "pasted" }),
        )
        .unwrap();
    assert_eq!(status(&added["state"], "infra.env"), "ok");
    assert_eq!(
        app.handle("open", &json!({ "path": ip })).unwrap()["items"][0]["v"],
        "9"
    );
    assert!(app
        .handle("addSessionKey", &json!({ "content": "hello" }))
        .is_err());

    text_formats_flow(&mut app, &root, &a, &b);
    robustness_flow(&mut app, &root, &a);
    git_flow(&mut app, &root, &a);

    let _ = std::fs::remove_dir_all(&root);
}

/// Files in other encodings, files changed behind the app's back, settings.
fn robustness_flow(app: &mut App, root: &std::path::Path, key: &str) {
    let dir = root.join("odd");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("binary.env"), b"\x7fELF\x00\x01\x02\xff\xfe\x00").unwrap();
    app.handle("setFolder", &json!({ "path": dir.to_string_lossy() }))
        .unwrap();
    let items =
        json!([{ "t": "kv", "k": "A", "v": "1" }, { "t": "kv", "k": "NAME", "v": "Zo\u{eb}" }]);
    let saved = app
        .handle(
            "create",
            &json!({ "dir": dir.to_string_lossy(), "name": "app.env", "items": items, "recipients": [key] }),
        )
        .unwrap();
    let state = saved["state"].clone();
    assert_eq!(status(&state, "binary.env"), "absent");
    let rev_of = |state: &Value, name: &str| -> Value {
        state["files"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["name"] == name)
            .unwrap()["rev"]
            .clone()
    };
    let p = dir.join("app.env").to_string_lossy().to_string();
    assert!(std::fs::read_to_string(&p).unwrap().starts_with("A=ENC["));

    // Someone else changes the file after it was opened here.
    let mine = saved["file"]["rev"].clone();
    // The file list carries the same fingerprint as the opened file.
    assert_eq!(rev_of(&saved["state"], "app.env"), mine);
    let theirs = json!([{ "t": "kv", "k": "A", "v": "theirs" }, { "t": "kv", "k": "NAME", "v": "Zo\u{eb}" }, { "t": "kv", "k": "EXTRA", "v": "x" }]);
    app.handle("save", &json!({ "path": p, "items": theirs }))
        .unwrap();
    let on_disk = std::fs::read(&p).unwrap();
    let ours =
        json!([{ "t": "kv", "k": "A", "v": "ours" }, { "t": "kv", "k": "NAME", "v": "Zo\u{eb}" }]);
    let listed = app.handle("state", &json!({})).unwrap();
    assert_ne!(
        rev_of(&listed, "app.env"),
        mine,
        "a change on disk changes the listed fingerprint"
    );
    let refused = app
        .handle("save", &json!({ "path": p, "items": ours, "rev": mine }))
        .unwrap();
    assert_eq!(refused, json!({ "conflict": true }));
    assert_eq!(
        std::fs::read(&p).unwrap(),
        on_disk,
        "a refused save writes nothing"
    );
    let forced = app
        .handle(
            "save",
            &json!({ "path": p, "items": ours, "rev": mine, "overwrite": true }),
        )
        .unwrap();
    assert_eq!(
        forced["file"]["items"], ours,
        "overwriting saves exactly the user's version"
    );
    // With the current fingerprint, saving goes through without asking.
    let again = app
        .handle(
            "save",
            &json!({ "path": p, "items": theirs, "rev": forced["file"]["rev"] }),
        )
        .unwrap();
    assert_eq!(again["file"]["items"], theirs);

    // Saving a key only appends: what is already in keys.txt stays byte for byte.
    let keys_txt = root.join("xdg/sops/age/keys.txt");
    std::fs::create_dir_all(keys_txt.parent().unwrap()).unwrap();
    let existing = "# my notes, kept as they are\r\nAGE-SECRET-KEY-NOT-A-REAL-ONE   \n\n# no newline at the end";
    std::fs::write(&keys_txt, existing).unwrap();
    let made = app
        .handle("generateKey", &json!({ "persist": true }))
        .unwrap();
    let after = std::fs::read_to_string(&keys_txt).unwrap();
    assert!(
        after.starts_with(existing),
        "existing content was rewritten"
    );
    let added = &after[existing.len()..];
    assert!(
        added.starts_with("\n# created: ")
            && added.contains(made["pub"].as_str().unwrap())
            && added.ends_with('\n')
    );
    assert_eq!(added.matches("AGE-SECRET-KEY-").count(), 1);
    std::fs::remove_file(&keys_txt).unwrap();

    // A new encrypted file: checked first, created only with content, never over an existing one.
    let d = dir.to_string_lossy().to_string();
    let check =
        |app: &mut App, name: &str| app.handle("checkNewFile", &json!({ "dir": d, "name": name }));
    for (name, why) in [
        ("../x.env", "without folders"),
        ("sub/x.env", "without folders"),
        ("app.env", "already exists"),
    ] {
        let err = check(app, name).unwrap_err();
        assert!(err.contains(why), "{name}: {err}");
    }
    let missing = app.handle(
        "checkNewFile",
        &json!({ "dir": root.join("nope").to_string_lossy(), "name": ".env" }),
    );
    assert!(missing.unwrap_err().contains("doesn't exist"));
    let ok = check(app, ".env.ci").unwrap();
    assert_eq!(
        (ok["hasRule"].as_bool(), ok["kind"].as_str()),
        (Some(false), Some("env"))
    );
    let any = check(app, "notes.txt").unwrap();
    assert_eq!(
        (any["format"].as_str(), any["kind"].as_str()),
        (Some("binary"), Some("text"))
    );
    assert_eq!(check(app, "config.yaml").unwrap()["format"], "yaml");
    assert!(
        !dir.join(".env.ci").exists(),
        "checking a name writes nothing"
    );
    let ci = json!([{ "t": "kv", "k": "CI_TOKEN", "v": "abc 123" }, { "t": "raw", "text": "# deploy" }, { "t": "kv", "k": "REGION", "v": "eu" }]);
    let create = |app: &mut App, name: &str, items: &Value, recipients: Value| {
        app.handle(
            "create",
            &json!({ "dir": d, "name": name, "items": items, "recipients": recipients }),
        )
    };
    assert!(create(app, ".env.ci", &json!([]), json!([key]))
        .unwrap_err()
        .contains("at least one variable"));
    assert!(create(app, ".env.ci", &ci, json!([]))
        .unwrap_err()
        .contains("recipient"));
    assert!(
        !dir.join(".env.ci").exists(),
        "a refused create writes nothing"
    );
    let made = create(app, ".env.ci", &ci, json!([key])).unwrap();
    assert_eq!(made["file"]["items"], ci);
    assert_eq!(made["file"]["kind"], "env");
    assert!(!std::fs::read_to_string(dir.join(".env.ci"))
        .unwrap()
        .contains("abc 123"));
    assert_eq!(status(&made["state"], ".env.ci"), "ok");
    let before = std::fs::read(dir.join(".env.ci")).unwrap();
    assert!(create(app, ".env.ci", &ci, json!([key]))
        .unwrap_err()
        .contains("already exists"));
    assert_eq!(
        std::fs::read(dir.join(".env.ci")).unwrap(),
        before,
        "an existing file is never replaced"
    );
    // With a matching .sops.yaml rule, no recipients are needed.
    let ruled = dir.join("ruled");
    std::fs::create_dir_all(&ruled).unwrap();
    std::fs::write(
        ruled.join(".sops.yaml"),
        format!("creation_rules:\n  - age: {key}\n"),
    )
    .unwrap();
    let r = ruled.to_string_lossy().to_string();
    let checked = app
        .handle("checkNewFile", &json!({ "dir": r, "name": "prod.env" }))
        .unwrap();
    assert_eq!(checked["hasRule"], true);
    let made = app
        .handle(
            "create",
            &json!({ "dir": r, "name": "prod.env", "items": [{ "t": "kv", "k": "A", "v": "1" }] }),
        )
        .unwrap();
    assert_eq!(made["file"]["recipients"][0]["key"], key);

    // The clipboard setting has a default, validates and is stored.
    assert_eq!(state["settings"]["clipboardClearSeconds"], 60);
    let state = app
        .handle("setSettings", &json!({ "clipboardClearSeconds": 15 }))
        .unwrap();
    assert_eq!(state["settings"]["clipboardClearSeconds"], 15);
    assert!(app
        .handle("setSettings", &json!({ "clipboardClearSeconds": 4000 }))
        .is_err());
    let fresh = App::new(Some(dir.clone()), vec![]);
    assert_eq!(fresh.clipboard_clear_seconds(), 15);
    app.handle("setSettings", &json!({ "clipboardClearSeconds": 0 }))
        .unwrap();
    assert_eq!(app.clipboard_clear_seconds(), 0);

    // Update checks: on by default, can be turned off on their own, and stay off.
    assert_eq!(state["settings"]["checkUpdates"], true);
    let state = app
        .handle("setSettings", &json!({ "checkUpdates": false }))
        .unwrap();
    assert_eq!(state["settings"]["checkUpdates"], false);
    assert_eq!(
        state["settings"]["clipboardClearSeconds"], 0,
        "changing one setting keeps the others"
    );
    let state = app
        .handle("setSettings", &json!({ "clipboardClearSeconds": 30 }))
        .unwrap();
    assert_eq!(
        state["settings"]["checkUpdates"], false,
        "changing one setting keeps the others"
    );
    assert!(!App::new(Some(dir.clone()), vec![]).check_updates());
    assert!(app
        .handle("setSettings", &json!({ "checkUpdates": "yes" }))
        .is_err());
    app.handle("setSettings", &json!({ "checkUpdates": true }))
        .unwrap();

    // The version the app reports is the one the build sets, and updates are judged against it.
    app.set_version("0.1.0-beta.4");
    assert_eq!(
        app.handle("state", &json!({})).unwrap()["version"],
        "0.1.0-beta.4"
    );
    let feed = json!([
        { "tag_name": "v0.1.0", "prerelease": false, "draft": false, "body": "" },
        { "tag_name": "v0.1.0-beta.5", "prerelease": true, "draft": false, "body": "" },
    ]);
    let picked = app
        .handle("pickUpdate", &json!({ "releases": feed }))
        .unwrap();
    assert_eq!(
        (picked["current"].as_str(), picked["update"]["tag"].as_str()),
        (Some("0.1.0-beta.4"), Some("v0.1.0"))
    );
    app.set_version("0.1.0");
    assert!(app
        .handle("pickUpdate", &json!({ "releases": feed }))
        .unwrap()["update"]
        .is_null());
}

/// Repositories, Git status, comparison with the last commit and .gitignore.
/// Skipped when git isn't installed; the app itself must not need it, which
/// the unit tests in the crate cover.
fn git_flow(app: &mut App, root: &std::path::Path, key: &str) {
    let work = root.join("work");
    let repo = work.join("shop");
    std::fs::create_dir_all(repo.join("config")).unwrap();
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .args([
                "-c",
                "user.name=test",
                "-c",
                "user.email=test@example.invalid",
                "-c",
                "core.autocrlf=false",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .current_dir(&repo)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    };
    if !git(&["init", "-q", "-b", "main"]) {
        eprintln!("git not installed: skipping the Git part of the test");
        return;
    }
    let write = |rel: &str, text: &str| std::fs::write(repo.join(rel), text).unwrap();
    write(".gitignore", "local.env\n");
    // Plaintext files, committed or not, aren't the app's business.
    write(".env.example", "API_KEY=\n");
    write("leak.env", "S=1\n");

    // Built part by part, so the separators match what the app reports on Windows.
    let path = |rel: &str| {
        rel.split('/')
            .fold(repo.clone(), |p, part| p.join(part))
            .to_string_lossy()
            .to_string()
    };
    app.handle("setFolder", &json!({ "path": work.to_string_lossy() }))
        .unwrap();
    let kv = |pairs: &[(&str, &str)]| -> Value {
        pairs
            .iter()
            .map(|(k, v)| json!({ "t": "kv", "k": k, "v": v }))
            .collect()
    };
    let create = |app: &mut App, dir: &std::path::Path, name: &str, content: Value| {
        let mut body = json!({ "dir": dir.to_string_lossy(), "name": name, "recipients": [key] });
        if content.is_string() {
            body["text"] = content;
        } else {
            body["items"] = content;
        }
        app.handle("create", &body).unwrap();
    };
    create(app, &repo, "local.env", kv(&[("L", "1")]));
    create(app, &work, "loose.env", kv(&[("Z", "1")]));
    create(
        app,
        &repo.join("config"),
        "prod.env",
        kv(&[("KEEP", "1"), ("CHANGE", "old"), ("DROP", "x")]),
    );
    create(
        app,
        &repo.join("config"),
        "app.yaml",
        json!("db:\n  host: db.internal\n  password: one\nport: 5432\n"),
    );
    let prod = path("config/prod.env");
    assert!(git(&[
        "add",
        ".gitignore",
        ".env.example",
        "leak.env",
        "config/prod.env",
        "config/app.yaml",
    ]));
    assert!(git(&["commit", "-q", "-m", "init"]));
    create(app, &repo, "new.env", kv(&[("N", "1")]));

    let state = app.handle("state", &json!({})).unwrap();
    assert_eq!(state["git"], true);
    assert_eq!(state["recent"][0], work.to_string_lossy().to_string());
    assert_eq!(state["repos"].as_array().unwrap().len(), 1);
    assert_eq!(state["repos"][0]["name"], "shop");
    assert_eq!(state["repos"][0]["branch"], "main");
    let find = |state: &Value, p: &str| {
        state["files"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["path"] == p)
            .cloned()
    };
    let file = |state: &Value, p: &str| find(state, p).unwrap();
    let f = file(&state, &prod);
    assert_eq!(
        (f["git"].as_str(), f["status"].as_str()),
        (Some("clean"), Some("ok"))
    );
    assert_eq!(f["repo"], repo.to_string_lossy().to_string());
    assert_eq!(
        f["rel"],
        std::path::Path::new("config")
            .join("prod.env")
            .to_string_lossy()
            .to_string()
    );
    assert!(find(&state, &path(".env.example")).is_none());
    assert!(find(&state, &path("leak.env")).is_none());
    let f = file(&state, &path("local.env"));
    assert_eq!(
        (f["git"].as_str(), f["status"].as_str()),
        (Some("ignored"), Some("ok"))
    );
    let f = file(&state, &path("new.env"));
    assert_eq!(
        (f["git"].as_str(), f["status"].as_str()),
        (Some("new"), Some("ok"))
    );
    let f = file(&state, &work.join("loose.env").to_string_lossy());
    assert!(f["repo"].is_null() && f["git"].is_null());
    assert_eq!(file(&state, &path("config/app.yaml"))["format"], "yaml");

    // Nothing changed yet; then edit and compare with the commit.
    let same = app.handle("changes", &json!({ "path": prod })).unwrap();
    assert_eq!(
        (
            same["committed"].as_bool(),
            same["changes"].as_array().unwrap().len(),
            same["unchanged"].as_u64()
        ),
        (Some(true), 0, Some(3))
    );
    let saved = app
        .handle("save", &json!({ "path": prod, "items": kv(&[("KEEP", "1"), ("CHANGE", "new"), ("ADDED", "y")]) }))
        .unwrap();
    assert_eq!(file(&saved["state"], &prod)["git"], "modified");
    let diff = app.handle("changes", &json!({ "path": prod })).unwrap();
    assert_eq!(
        diff["changes"],
        json!([
            { "k": "CHANGE", "kind": "changed", "old": "old", "new": "new" },
            { "k": "ADDED", "kind": "added", "new": "y" },
            { "k": "DROP", "kind": "removed", "old": "x" },
        ])
    );
    assert_eq!(diff["unchanged"], 1);

    // Other formats compare line by line.
    let yaml = path("config/app.yaml");
    let opened = app.handle("open", &json!({ "path": yaml })).unwrap();
    let edited = opened["text"]
        .as_str()
        .unwrap()
        .replace("password: one", "password: two");
    app.handle(
        "save",
        &json!({ "path": yaml, "text": edited, "rev": opened["rev"] }),
    )
    .unwrap();
    let diff = app.handle("changes", &json!({ "path": yaml })).unwrap();
    assert_eq!(
        (diff["kind"].as_str(), diff["changed"].as_u64()),
        (Some("text"), Some(2))
    );
    let changed: Vec<(String, String)> = diff["lines"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|l| l["kind"] == "added" || l["kind"] == "removed")
        .map(|l| {
            (
                l["kind"].as_str().unwrap().to_string(),
                l["text"].as_str().unwrap().trim().to_string(),
            )
        })
        .collect();
    assert_eq!(
        changed,
        [
            ("removed".to_string(), "password: one".to_string()),
            ("added".to_string(), "password: two".to_string())
        ]
    );

    // A file that was never committed is all additions; one outside a repository can't be compared.
    let fresh = app
        .handle("changes", &json!({ "path": path("new.env") }))
        .unwrap();
    assert_eq!(fresh["committed"], false);
    assert_eq!(
        fresh["changes"],
        json!([{ "k": "N", "kind": "added", "new": "1" }])
    );
    let err = app
        .handle(
            "changes",
            &json!({ "path": work.join("loose.env").to_string_lossy() }),
        )
        .unwrap_err();
    assert!(err.contains("isn't in a Git repository"), "{err}");
}

/// YAML, JSON, INI and plain text files, encrypted by sops itself as a
/// project would: recognised by content, opened as text, and saved without
/// losing their recipients or options.
fn text_formats_flow(app: &mut App, root: &std::path::Path, a: &str, b: &str) {
    let dir = root.join("formats");
    std::fs::create_dir_all(dir.join("deep").join("er").join("still").join("deeper")).unwrap();
    std::fs::create_dir_all(dir.join("vendor")).unwrap();
    std::fs::create_dir_all(dir.join("strict")).unwrap();
    let state = app.handle("state", &json!({})).unwrap();
    let sops = state["sops"]["path"].as_str().unwrap().to_string();
    let keys = root.join("moved-keys.txt");
    let run = |cwd: &std::path::Path, args: &[&str], stdin: &[u8]| -> Vec<u8> {
        use std::io::Write;
        let mut child = std::process::Command::new(&sops)
            .args(args)
            .current_dir(cwd)
            .env("SOPS_AGE_KEY_FILE", &keys)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(stdin).unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        out.stdout
    };
    // Encrypts like `sops encrypt --age <a> file > file` would.
    let encrypt = |rel: &[&str], types: (&str, &str), extra: &[&str], plain: &[u8]| {
        let target = rel.iter().fold(dir.clone(), |p, part| p.join(part));
        let cwd = target.parent().unwrap().to_path_buf();
        let name = target.file_name().unwrap().to_string_lossy().to_string();
        let mut args = vec!["encrypt", "--input-type", types.0, "--output-type", types.1];
        if !extra.contains(&"--rule") {
            args.extend(["--age", a]);
        }
        args.extend(extra.iter().filter(|x| **x != "--rule"));
        args.extend(["--filename-override", &name]);
        std::fs::write(&target, run(&cwd, &args, plain)).unwrap();
        target.to_string_lossy().to_string()
    };
    let decrypt = |path: &str, types: (&str, &str)| -> String {
        String::from_utf8(run(
            &dir,
            &[
                "decrypt",
                "--input-type",
                types.0,
                "--output-type",
                types.1,
                path,
            ],
            b"",
        ))
        .unwrap()
    };
    let acl = encrypt(
        &["valkey-users.enc.acl"],
        ("binary", "json"),
        &[],
        b"user default off\r\nuser alice on >pw-1 ~* +@all\r\n",
    );
    let yaml = encrypt(
        &["config.yaml"],
        ("yaml", "yaml"),
        &["--encrypted-regex", "^password$"],
        b"db:\n  host: db.internal\n  password: hunter2\n",
    );
    let jsonf = encrypt(
        &["app.json"],
        ("json", "json"),
        &[],
        br#"{"api": {"token": "t-1"}}"#,
    );
    let ini = encrypt(&["db.ini"], ("ini", "ini"), &[], b"[db]\npass = p1\n");
    let raw = encrypt(
        &["raw.secret"],
        ("binary", "json"),
        &[],
        b"\xff\x00\x01binary",
    );
    encrypt(
        &["deep", "er", "still", "deeper", "far.yaml"],
        ("yaml", "yaml"),
        &[],
        b"a: 1\n",
    );
    encrypt(&["vendor", "dep.yaml"], ("yaml", "yaml"), &[], b"a: 1\n");
    std::fs::write(
        dir.join("strict").join(".sops.yaml"),
        format!("creation_rules:\n  - age: {a}\n    mac_only_encrypted: true\n"),
    )
    .unwrap();
    let strict = encrypt(
        &["strict", "only-mac.yaml"],
        ("yaml", "yaml"),
        &["--rule"],
        b"x: 1\n",
    );
    // Files that merely look a bit like sops files.
    std::fs::write(
        dir.join("decoy.json"),
        r#"{"sops": {"version": "3"}, "mac": "ENC[AES256_GCM,data:x]"}"#,
    )
    .unwrap();
    std::fs::write(dir.join("plain.yaml"), "password: hunter2\n").unwrap();

    let state = app
        .handle("setFolder", &json!({ "path": dir.to_string_lossy() }))
        .unwrap();
    let format = |state: &Value, rel: &str| -> String {
        state["files"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["name"] == rel)
            .map(|f| f["format"].as_str().unwrap().to_string())
            .unwrap_or_else(|| "absent".into())
    };
    for (rel, expected) in [
        ("valkey-users.enc.acl", "binary"),
        ("config.yaml", "yaml"),
        ("app.json", "json"),
        ("db.ini", "ini"),
        ("raw.secret", "binary"),
        ("decoy.json", "absent"),
        ("plain.yaml", "absent"),
    ] {
        assert_eq!(format(&state, rel), expected, "{rel}");
    }
    let far = std::path::Path::new("deep")
        .join("er")
        .join("still")
        .join("deeper")
        .join("far.yaml");
    let far = far.to_string_lossy().to_string();
    let dep = std::path::Path::new("vendor")
        .join("dep.yaml")
        .to_string_lossy()
        .to_string();
    assert_eq!(format(&state, &far), "absent", "deeper than the scan depth");
    assert_eq!(
        format(&state, &dep),
        "absent",
        "vendor is skipped by default"
    );

    // Scan depth and skipped folders are settings.
    let state = app
        .handle("setSettings", &json!({ "scanDepth": 4 }))
        .unwrap();
    assert_eq!(state["settings"]["scanDepth"], 4);
    assert_eq!(format(&state, &far), "yaml");
    for bad in [0, 11] {
        assert!(app
            .handle("setSettings", &json!({ "scanDepth": bad }))
            .is_err());
    }
    let state = app
        .handle("setSettings", &json!({ "skipDirs": ["node_modules", " "] }))
        .unwrap();
    assert_eq!(state["settings"]["skipDirs"], json!(["node_modules"]));
    assert_eq!(format(&state, &dep), "yaml");
    assert!(app
        .handle("setSettings", &json!({ "skipDirs": ["a/b"] }))
        .is_err());
    let defaults = state["settings"]["defaultSkipDirs"].clone();
    let state = app
        .handle(
            "setSettings",
            &json!({ "skipDirs": defaults, "scanDepth": 3 }),
        )
        .unwrap();
    assert_eq!(state["settings"]["skipDirs"], defaults);
    assert_eq!(format(&state, &dep), "absent");
    assert_eq!(App::new(Some(dir.clone()), vec![]).scan_depth(), 3);

    // A text file opens as text and saves in place, Windows line endings kept.
    let opened = app.handle("open", &json!({ "path": acl })).unwrap();
    assert_eq!(
        (opened["kind"].as_str(), opened["format"].as_str()),
        (Some("text"), Some("binary"))
    );
    assert_eq!(
        opened["text"],
        "user default off\r\nuser alice on >pw-1 ~* +@all\r\n"
    );
    assert!(opened["reencrypt"].is_null());
    let saved = app
        .handle("save", &json!({ "path": acl, "text": "user default off\nuser bob on >pw-2\n", "rev": opened["rev"] }))
        .unwrap();
    assert_eq!(
        saved["file"]["text"],
        "user default off\r\nuser bob on >pw-2\r\n"
    );
    assert_eq!(
        decrypt(&acl, ("json", "binary")),
        "user default off\r\nuser bob on >pw-2\r\n"
    );
    assert!(!std::fs::read_to_string(&acl).unwrap().contains("pw-2"));
    assert_eq!(saved["file"]["recipients"][0]["key"], a);
    // A stale fingerprint is a conflict, as for env files.
    let stale = app
        .handle(
            "save",
            &json!({ "path": acl, "text": "x\n", "rev": opened["rev"] }),
        )
        .unwrap();
    assert_eq!(stale, json!({ "conflict": true }));
    assert!(
        app.handle("save", &json!({ "path": acl, "items": [] }))
            .is_err(),
        "a text file takes text"
    );

    // YAML is encrypted again with its options: only `password` is encrypted.
    let opened = app.handle("open", &json!({ "path": yaml })).unwrap();
    let text = opened["text"]
        .as_str()
        .unwrap()
        .replace("hunter2", "hunter3");
    app.handle("save", &json!({ "path": yaml, "text": text }))
        .unwrap();
    let on_disk = std::fs::read_to_string(&yaml).unwrap();
    assert!(on_disk.contains("host: db.internal") && !on_disk.contains("hunter3"));
    assert!(on_disk.contains("encrypted_regex: ^password$"));
    assert!(decrypt(&yaml, ("yaml", "yaml")).contains("password: hunter3"));
    let both = app
        .handle(
            "setRecipients",
            &json!({ "path": yaml, "recipients": [a, b] }),
        )
        .unwrap();
    assert_eq!(both["file"]["recipients"].as_array().unwrap().len(), 2);
    assert!(std::fs::read_to_string(&yaml)
        .unwrap()
        .contains("encrypted_regex: ^password$"));

    for (path, from, to) in [(&jsonf, "t-1", "t-2"), (&ini, "p1", "p2")] {
        let opened = app.handle("open", &json!({ "path": path })).unwrap();
        let text = opened["text"].as_str().unwrap().replace(from, to);
        let saved = app
            .handle("save", &json!({ "path": path, "text": text }))
            .unwrap();
        assert!(
            saved["file"]["text"].as_str().unwrap().contains(to),
            "{path}"
        );
        assert!(
            !std::fs::read_to_string(path).unwrap().contains(to),
            "{path}"
        );
    }
    let broken = app.handle("save", &json!({ "path": jsonf, "text": "{ not json" }));
    assert!(broken.is_err(), "sops refuses invalid JSON");
    assert!(
        std::fs::read_to_string(&jsonf)
            .unwrap()
            .contains("\"sops\""),
        "and the file stays as it was"
    );

    // What the app can't keep, it doesn't touch.
    let opened = app.handle("open", &json!({ "path": strict })).unwrap();
    assert!(opened["reencrypt"]
        .as_str()
        .unwrap()
        .contains("mac_only_encrypted"));
    let before = std::fs::read(&strict).unwrap();
    let err = app
        .handle("save", &json!({ "path": strict, "text": "x: 2\n" }))
        .unwrap_err();
    assert!(err.contains("mac_only_encrypted"), "{err}");
    assert!(app
        .handle(
            "setRecipients",
            &json!({ "path": strict, "recipients": [a, b] })
        )
        .is_err());
    assert_eq!(std::fs::read(&strict).unwrap(), before);
    let err = app.handle("open", &json!({ "path": raw })).unwrap_err();
    assert!(err.contains("binary data"), "{err}");

    // New files take their format from the name.
    let new_text = |app: &mut App, name: &str, text: &str| {
        app.handle(
            "create",
            &json!({ "dir": dir.to_string_lossy(), "name": name, "text": text, "recipients": [a] }),
        )
    };
    assert!(new_text(app, "empty.yaml", " \n")
        .unwrap_err()
        .contains("content"));
    let made = new_text(app, "service.yaml", "token: abc\n").unwrap();
    assert_eq!(
        (
            made["file"]["format"].as_str(),
            made["file"]["kind"].as_str()
        ),
        (Some("yaml"), Some("text"))
    );
    assert_eq!(format(&made["state"], "service.yaml"), "yaml");
    let made = new_text(app, "users.acl", "user carol on\n").unwrap();
    assert_eq!(made["file"]["format"], "binary");
    assert_eq!(made["file"]["text"], "user carol on\n");
    assert!(app
        .handle("create", &json!({ "dir": dir.to_string_lossy(), "name": "x.json", "items": [{ "t": "kv", "k": "A", "v": "1" }], "recipients": [a] }))
        .unwrap_err()
        .contains("couldn't be read"));
}
