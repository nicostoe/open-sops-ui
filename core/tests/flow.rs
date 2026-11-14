//! End-to-end test against a real `sops` binary with throwaway keys.
//! Everything runs inside a temp folder; the user's own keys.txt is never read.
use serde_json::{json, Value};
use sops_env_core::App;
use std::path::PathBuf;

fn pubs(state: &Value, kind: &str) -> Vec<String> {
    state["keys"].as_array().unwrap().iter().filter(|k| k["kind"] == kind)
        .flat_map(|k| k["pubs"].as_array().unwrap().clone())
        .map(|p| p.as_str().unwrap().to_string()).collect()
}

fn status(state: &Value, name: &str) -> String {
    state["files"].as_array().unwrap().iter().find(|f| f["name"] == name)
        .map(|f| f["status"].as_str().unwrap().to_string()).unwrap_or_else(|| "absent".into())
}

#[test]
fn full_flow() {
    let root = std::env::temp_dir().join(format!("sops-env-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let demo = root.join("demo");
    std::fs::create_dir_all(&demo).unwrap();
    std::env::set_var("XDG_CONFIG_HOME", root.join("xdg"));
    std::env::set_var("SOPS_AGE_KEY_FILE", root.join("none.txt"));
    std::env::remove_var("SOPS_AGE_KEY");

    // SOPS_ENV_TEST_EDITOR lets the same test exercise the desktop binary's editor mode.
    let helper = std::env::var_os("SOPS_ENV_TEST_EDITOR")
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
    let a = app.handle("generateKey", &json!({ "persist": true })).unwrap()["pub"].as_str().unwrap().to_string();
    let b = app.handle("generateKey", &json!({ "label": "homelab" })).unwrap()["pub"].as_str().unwrap().to_string();
    let keys_txt = root.join("xdg/sops/age/keys.txt");
    assert_eq!(std::fs::read_to_string(&keys_txt).unwrap().matches("AGE-SECRET-KEY-").count(), 1);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&keys_txt).unwrap().permissions().mode() & 0o777, 0o600);
    }

    // Plaintext file: needs recipients, then encrypts in place.
    let file = demo.join(".env.prod.enc");
    let p = file.to_string_lossy().to_string();
    std::fs::write(&file, "DB=postgres://u:p@h/db\n# payment\nSTRIPE=sk_1\nJWT=a=b\n").unwrap();
    assert_eq!(status(&app.handle("state", &json!({})).unwrap(), ".env.prod.enc"), "plain");
    let opened = app.handle("open", &json!({ "path": p })).unwrap();
    assert_eq!(opened["encrypted"], false);
    let items = opened["items"].clone();
    let err = app.handle("save", &json!({ "path": p, "items": items })).unwrap_err();
    assert!(err.contains("recipient"), "{err}");
    let saved = app.handle("save", &json!({ "path": p, "items": items, "recipients": [a] })).unwrap();
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
    let saved = app.handle("save", &json!({ "path": p, "items": edited })).unwrap();
    assert_eq!(saved["file"]["items"], json!(edited));
    let after = std::fs::read_to_string(&file).unwrap();
    let line = |text: &str, key: &str| text.lines().find(|l| l.starts_with(key)).unwrap().to_string();
    assert_eq!(line(&before, "DB="), line(&after, "DB="));
    assert_eq!(line(&before, "JWT="), line(&after, "JWT="));
    assert_ne!(line(&before, "STRIPE="), line(&after, "STRIPE="));

    // Saving without changes is fine; invalid input is rejected.
    app.handle("save", &json!({ "path": p, "items": edited })).unwrap();
    let dup = json!([{ "t": "kv", "k": "A", "v": "1" }, { "t": "kv", "k": "A", "v": "2" }]);
    assert!(app.handle("save", &json!({ "path": p, "items": dup })).unwrap_err().contains("twice"));

    // Recipients: add B, then check the .sops.yaml drift warning and fix.
    let both = app.handle("setRecipients", &json!({ "path": p, "recipients": [a, b] })).unwrap();
    assert_eq!(both["file"]["recipients"].as_array().unwrap().len(), 2);
    assert!(app.handle("setRecipients", &json!({ "path": p, "recipients": ["nope"] })).is_err());
    std::fs::write(demo.join(".sops.yaml"), format!("creation_rules:\n  - path_regex: \\.env\\.prod.*\n    age: {a}\n")).unwrap();
    let state = app.handle("state", &json!({})).unwrap();
    let f = state["files"].as_array().unwrap().iter().find(|f| f["name"] == ".env.prod.enc").unwrap();
    assert_eq!(f["drift"]["extra"][0], b);
    let state = app.handle("applyRule", &json!({ "path": p })).unwrap();
    let f = state["files"].as_array().unwrap().iter().find(|f| f["name"] == ".env.prod.enc").unwrap();
    assert!(f["drift"].is_null());
    assert_eq!(f["recipients"].as_array().unwrap().len(), 1);

    // A plaintext file matched by a rule encrypts without choosing recipients.
    let ruled = demo.join(".env.prod.new");
    std::fs::write(&ruled, "Z=1\n").unwrap();
    let rp = ruled.to_string_lossy().to_string();
    app.handle("save", &json!({ "path": rp, "items": [{ "t": "kv", "k": "Z", "v": "1" }] })).unwrap();

    // A file only B can open becomes locked once B is removed.
    let infra = demo.join("infra.env");
    std::fs::write(&infra, "X=9\n").unwrap();
    let ip = infra.to_string_lossy().to_string();
    app.handle("save", &json!({ "path": ip, "items": [{ "t": "kv", "k": "X", "v": "9" }], "recipients": [b] })).unwrap();
    let state = app.handle("state", &json!({})).unwrap();
    assert_eq!(status(&state, "infra.env"), "ok");
    let session_id = state["keys"].as_array().unwrap().iter().find(|k| k["kind"] == "session").unwrap()["id"].as_str().unwrap().to_string();

    // Persist B: it moves into keys.txt next to A, and survives a restart.
    let state = app.handle("persistKey", &json!({ "id": session_id })).unwrap();
    assert_eq!(pubs(&state, "default"), vec![a.clone(), b.clone()]);
    assert!(pubs(&state, "session").is_empty());
    assert_eq!(std::fs::read_to_string(&keys_txt).unwrap().matches("AGE-SECRET-KEY-").count(), 2);

    // Saved recipients are validated and persisted.
    assert!(app.handle("bookAdd", &json!({ "name": "x", "key": "nope" })).is_err());
    app.handle("bookAdd", &json!({ "name": "Anna", "key": b })).unwrap();

    // Fresh start with keys.txt removed: files are locked, opening explains why.
    std::fs::rename(&keys_txt, root.join("moved-keys.txt")).unwrap();
    let mut app = App::new(Some(demo.clone()), vec![], helper);
    let state = app.handle("state", &json!({})).unwrap();
    assert_eq!(state["book"][0]["name"], "Anna");
    assert_eq!(status(&state, "infra.env"), "locked");
    assert!(app.handle("open", &json!({ "path": ip })).unwrap_err().contains("None of your loaded keys"));

    // A custom key path and a pasted key both unlock it again.
    let moved = root.join("moved-keys.txt").to_string_lossy().to_string();
    let state = app.handle("addKeyPath", &json!({ "path": moved, "label": "work" })).unwrap();
    assert_eq!(status(&state, "infra.env"), "ok");
    let state = app.handle("removeKey", &json!({ "id": format!("path:{moved}") })).unwrap();
    assert_eq!(status(&state, "infra.env"), "locked");
    let content = std::fs::read_to_string(root.join("moved-keys.txt")).unwrap();
    let added = app.handle("addSessionKey", &json!({ "content": content, "label": "pasted" })).unwrap();
    assert_eq!(status(&added["state"], "infra.env"), "ok");
    assert_eq!(app.handle("open", &json!({ "path": ip })).unwrap()["items"][0]["v"], "9");
    assert!(app.handle("addSessionKey", &json!({ "content": "hello" })).is_err());

    let _ = std::fs::remove_dir_all(&root);
}
