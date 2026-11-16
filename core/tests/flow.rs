//! End-to-end test against a real `sops` binary with throwaway keys.
//! Everything runs inside a temp folder; the user's own keys.txt is never read.
use open_sops_ui_core::App;
use serde_json::{json, Value};
use std::path::PathBuf;

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

    // OPEN_SOPS_UI_TEST_EDITOR lets the same test exercise the desktop binary's editor mode.
    let helper = std::env::var_os("OPEN_SOPS_UI_TEST_EDITOR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_edit-helper")));
    let mut app = App::new(Some(demo.clone()), vec![], helper.clone());
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

    // Plaintext file: needs recipients, then encrypts in place.
    let file = demo.join(".env.prod.enc");
    let p = file.to_string_lossy().to_string();
    std::fs::write(
        &file,
        "DB=postgres://u:p@h/db\n# payment\nSTRIPE=sk_1\nJWT=a=b\n",
    )
    .unwrap();
    assert_eq!(
        status(&app.handle("state", &json!({})).unwrap(), ".env.prod.enc"),
        "plain"
    );
    let opened = app.handle("open", &json!({ "path": p })).unwrap();
    assert_eq!(opened["encrypted"], false);
    let items = opened["items"].clone();
    let err = app
        .handle("save", &json!({ "path": p, "items": items }))
        .unwrap_err();
    assert!(err.contains("recipient"), "{err}");
    let saved = app
        .handle(
            "save",
            &json!({ "path": p, "items": items, "recipients": [a] }),
        )
        .unwrap();
    assert_eq!(saved["file"]["encrypted"], true);
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

    // A plaintext file matched by a rule encrypts without choosing recipients.
    let ruled = demo.join(".env.prod.new");
    std::fs::write(&ruled, "Z=1\n").unwrap();
    let rp = ruled.to_string_lossy().to_string();
    app.handle(
        "save",
        &json!({ "path": rp, "items": [{ "t": "kv", "k": "Z", "v": "1" }] }),
    )
    .unwrap();

    // A file only B can open becomes locked once B is removed.
    let infra = demo.join("infra.env");
    std::fs::write(&infra, "X=9\n").unwrap();
    let ip = infra.to_string_lossy().to_string();
    app.handle(
        "save",
        &json!({ "path": ip, "items": [{ "t": "kv", "k": "X", "v": "9" }], "recipients": [b] }),
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

    // Fresh start with keys.txt removed: files are locked, opening explains why.
    std::fs::rename(&keys_txt, root.join("moved-keys.txt")).unwrap();
    let mut app = App::new(Some(demo.clone()), vec![], helper);
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

    git_flow(&mut app, &root, &a);

    let _ = std::fs::remove_dir_all(&root);
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
    write(".env.example", "API_KEY=\n");
    write("local.env", "L=1\n");
    write("leak.env", "S=1\n");
    write("config/prod.env", "KEEP=1\nCHANGE=old\nDROP=x\n");
    std::fs::write(work.join("loose.env"), "Z=1\n").unwrap();

    let path = |rel: &str| repo.join(rel).to_string_lossy().to_string();
    app.handle("setFolder", &json!({ "path": work.to_string_lossy() }))
        .unwrap();
    let kv = |pairs: &[(&str, &str)]| -> Value {
        pairs
            .iter()
            .map(|(k, v)| json!({ "t": "kv", "k": k, "v": v }))
            .collect()
    };
    let prod = path("config/prod.env");
    app.handle(
        "save",
        &json!({ "path": prod, "items": kv(&[("KEEP", "1"), ("CHANGE", "old"), ("DROP", "x")]), "recipients": [key] }),
    )
    .unwrap();
    assert!(git(&[
        "add",
        ".gitignore",
        ".env.example",
        "leak.env",
        "config/prod.env"
    ]));
    assert!(git(&["commit", "-q", "-m", "init"]));
    write("new.env", "N=1\n");

    let state = app.handle("state", &json!({})).unwrap();
    assert_eq!(state["git"], true);
    assert_eq!(state["recent"][0], work.to_string_lossy().to_string());
    assert_eq!(state["repos"].as_array().unwrap().len(), 1);
    assert_eq!(state["repos"][0]["name"], "shop");
    assert_eq!(state["repos"][0]["branch"], "main");
    let file = |state: &Value, p: &str| {
        state["files"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["path"] == p)
            .unwrap()
            .clone()
    };
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
    assert_eq!(file(&state, &path(".env.example"))["status"], "plain");
    assert_eq!(file(&state, &path("leak.env"))["status"], "plain-tracked");
    let f = file(&state, &path("local.env"));
    assert_eq!(
        (f["git"].as_str(), f["status"].as_str()),
        (Some("ignored"), Some("plain"))
    );
    let f = file(&state, &path("new.env"));
    assert_eq!(
        (f["git"].as_str(), f["status"].as_str()),
        (Some("new"), Some("plain-risk"))
    );
    let f = file(&state, &work.join("loose.env").to_string_lossy());
    assert!(f["repo"].is_null() && f["git"].is_null());
    assert_eq!(f["status"], "plain");

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

    // Ignoring adds one anchored line, once.
    let state = app
        .handle("ignore", &json!({ "path": path("new.env") }))
        .unwrap();
    let f = file(&state, &path("new.env"));
    assert_eq!(
        (f["git"].as_str(), f["status"].as_str()),
        (Some("ignored"), Some("plain"))
    );
    app.handle("ignore", &json!({ "path": path("new.env") }))
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(repo.join(".gitignore")).unwrap(),
        "local.env\n/new.env\n"
    );
}
