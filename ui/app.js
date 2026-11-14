const T = window.__TAURI__;
const $ = (s, r = document) => r.querySelector(s);
const ICONS = {
  lock: '<rect x="4" y="11" width="16" height="10" rx="2"/><path d="M8 11V7a4 4 0 0 1 8 0v4"/>',
  unlock: '<rect x="4" y="11" width="16" height="10" rx="2"/><path d="M8 11V7a4 4 0 0 1 7.5-2"/>',
  key: '<circle cx="8" cy="15" r="4"/><path d="M11 12l9-9M16 7l3 3"/>',
  users: '<circle cx="9" cy="8" r="3.5"/><path d="M2.5 20a6.5 6.5 0 0 1 13 0M16 4.8a3.5 3.5 0 0 1 0 6.4M18 14.5a6.5 6.5 0 0 1 3.5 5.5"/>',
  sun: '<circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M2 12h2M20 12h2M5 5l1.5 1.5M17.5 17.5L19 19M19 5l-1.5 1.5M6.5 17.5L5 19"/>',
  moon: '<path d="M20 14.5A8.5 8.5 0 0 1 9.5 4 8.5 8.5 0 1 0 20 14.5z"/>',
  monitor: '<rect x="3" y="4" width="18" height="12" rx="2"/><path d="M8 20h8M12 16v4"/>',
  eye: '<path d="M2 12s3.6-7 10-7 10 7 10 7-3.6 7-10 7S2 12 2 12z"/><circle cx="12" cy="12" r="3"/>',
  eyeoff: '<path d="M3 3l18 18M10.6 5.1A10.6 10.6 0 0 1 12 5c6.4 0 10 7 10 7a17.6 17.6 0 0 1-3.2 4M6.4 6.5A17.3 17.3 0 0 0 2 12s3.6 7 10 7a9.7 9.7 0 0 0 4.2-.9M9.9 9.9a3 3 0 0 0 4.2 4.2"/>',
  copy: '<rect x="9" y="9" width="12" height="12" rx="2"/><path d="M5 15V5a2 2 0 0 1 2-2h10"/>',
  trash: '<path d="M4 7h16M10 11v6M14 11v6M6 7l1 13h10l1-13M9 7V4h6v3"/>',
  plus: '<path d="M12 5v14M5 12h14"/>',
  folder: '<path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>',
  file: '<path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z"/><path d="M14 3v5h5"/>',
  x: '<path d="M6 6l12 12M18 6L6 18"/>',
  up: '<path d="M12 19V5M6 11l6-6 6 6"/>',
  alert: '<path d="M12 3l9.5 17h-19z"/><path d="M12 10v4M12 17.5v.1"/>',
};
const ic = (n) => `<svg viewBox="0 0 24 24" aria-hidden="true">${ICONS[n]}</svg>`;
const esc = (s) => String(s).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));
const short = (k) => k.slice(0, 10) + "…" + k.slice(-6);
document.querySelectorAll("[data-ic]").forEach((e) => (e.innerHTML = ic(e.dataset.ic)));

let S = null;          // server state
let cur = null;        // open file: { path, encrypted, items, orig, recipients, hasRule }
const shown = new Set(); // revealed row indexes

async function api(action, body = {}) {
  try { return await T.core.invoke("api", { action, body }); }
  catch (e) { throw new Error(typeof e === "string" ? e : "Something went wrong."); }
}
let toastTimer;
function toast(msg, bad = false) {
  const t = $("#toast");
  t.textContent = msg; t.className = "show" + (bad ? " bad" : "");
  clearTimeout(toastTimer); toastTimer = setTimeout(() => (t.className = ""), bad ? 5000 : 2200);
}
const fail = (e) => toast(e.message, true);
async function copy(text, what) {
  try { await T.clipboardManager.writeText(text); toast(what + " copied"); } catch { toast("Couldn't copy to the clipboard.", true); }
}

// ---- theme ---------------------------------------------------------------
const media = matchMedia("(prefers-color-scheme: dark)");
function theme() { try { return localStorage.getItem("theme") || "system"; } catch { return "system"; } }
function applyTheme() {
  const t = theme();
  document.documentElement.dataset.theme = t === "system" ? (media.matches ? "dark" : "light") : t;
  const b = $("#themeBtn");
  b.innerHTML = ic({ system: "monitor", light: "sun", dark: "moon" }[t]);
  b.title = b.ariaLabel = `Theme: ${t}`;
}
$("#themeBtn").onclick = () => {
  const next = { system: "light", light: "dark", dark: "system" }[theme()];
  try { localStorage.setItem("theme", next); } catch { /* not persisted */ }
  applyTheme();
};
media.addEventListener("change", applyTheme);
applyTheme();

// ---- main view -----------------------------------------------------------
function setState(s) {
  S = s;
  $("#folderBtn").textContent = "‎" + S.folder;
  const hasKey = S.keys.some((k) => k.pubs.length);
  $("#keysDot").className = hasKey ? "" : "dot";
  const banner = $("#banner");
  banner.style.display = S.sops.path && S.sops.supported ? "none" : "block";
  const brew = S.os === "darwin" ? ` (<span class="mono">brew install sops</span>)` : "";
  const from = `from <span class="mono">github.com/getsops/sops/releases</span>`;
  banner.innerHTML = S.sops.path
    ? `sops ${esc(S.sops.version)} is too old; saving may fail. Install sops 3.10 or newer${brew} ${from}, then restart this app.`
    : `sops wasn't found. Install it${brew} ${from}` +
      (S.appDir ? `, or put the sops binary in <span class="mono">${esc(S.appDir)}</span>` : "") + `. Then restart this app.`;
  renderFiles();
}
function isDirty() {
  return cur && JSON.stringify(cur.items) !== cur.orig;
}
function renderFiles() {
  const icon = { ok: "lock", locked: "lock", plain: "unlock", "plain-risk": "unlock", missing: "file" };
  $("#files").innerHTML = S.files.map((f, i) =>
    `<button class="file s-${f.status} ${cur && cur.path === f.path ? "active" : ""}" data-i="${i}" title="${esc(f.note)}">
      ${ic(icon[f.status])}<span class="mono">${esc(f.name)}</span></button>`).join("") +
    (S.files.length ? "" : `<div class="muted small" style="padding:6px 8px">No env files in this folder.</div>`) +
    `<button class="file addfile" id="addFile">${ic("plus")}<span>Open file</span></button>` +
    (S.sops.path ? `<div class="sopsinfo" title="${esc(S.sops.path)}">sops ${esc(S.sops.version)} · ${S.sops.source}</div>` : "");
  $("#files").querySelectorAll("[data-i]").forEach((b) => (b.onclick = () => openFile(S.files[b.dataset.i])));
  $("#addFile").onclick = async () => {
    const p = await pick("file", "Open env file", S.folder);
    if (!p) return;
    try {
      setState(await api("addFile", { path: p }));
      openFile(S.files.find((f) => f.path === p));
    } catch (e) { fail(e); }
  };
}
async function openFile(f) {
  if (!f) return;
  if (isDirty() && !(await ask("Discard unsaved changes?", "Discard"))) return;
  shown.clear();
  if (f.status === "locked" || f.status === "missing") { cur = { path: f.path, blocked: f }; renderFiles(); return renderEditor(); }
  try {
    const d = await api("open", { path: f.path });
    cur = { path: f.path, ...d, orig: JSON.stringify(d.items) };
  } catch (e) { cur = null; fail(e); }
  renderFiles(); renderEditor();
}
function meta() { return S.files.find((f) => cur && f.path === cur.path); }

function renderEditor() {
  const el = $("#editor");
  if (!cur) {
    el.innerHTML = `<div class="empty">${ic("lock")}<h2>Open an env file</h2>
      <p>Pick a file on the left to view and edit its values. Encrypted files are decrypted in memory only.</p></div>`;
    return;
  }
  const m = meta() || cur.blocked;
  if (cur.blocked) {
    const want = (m.recipients || []).map((k) => `<div class="mono small">${esc(k)}</div>`).join("");
    el.innerHTML = `<div class="empty">${ic("key")}<h2>${m.status === "missing" ? "File not found" : "No matching key"}</h2>
      ${m.status === "missing" ? "" : `<p>None of your loaded keys can open <span class="mono">${esc(m.name)}</span>. It's encrypted for:</p>${want}
      <p><button class="btn" id="goKeys">${ic("key")}Add a key</button></p>`}</div>`;
    if ($("#goKeys")) $("#goKeys").onclick = keysDialog;
    return;
  }
  const orig = new Map(JSON.parse(cur.orig).filter((i) => i.t === "kv").map((i) => [i.k, i.v]));
  const rows = cur.items.map((it, i) => {
    if (it.t !== "kv") return "";
    const cls = !orig.has(it.k) ? "added" : orig.get(it.k) !== it.v ? "changed" : "";
    const vis = shown.has(i);
    return `<div class="row ${cls}" data-i="${i}"><span class="mark"></span>
      <input class="k" type="text" value="${esc(it.k)}" placeholder="NAME" spellcheck="false" aria-label="Variable name">
      <input class="val" type="${vis ? "text" : "password"}" value="${esc(it.v)}" placeholder="value" spellcheck="false" autocomplete="off" aria-label="Value of ${esc(it.k)}">
      <span class="acts">
        <button class="btn icon" data-a="eye" title="${vis ? "Hide" : "Reveal"}" aria-label="${vis ? "Hide" : "Reveal"} value">${ic(vis ? "eyeoff" : "eye")}</button>
        <button class="btn icon" data-a="copy" title="Copy value" aria-label="Copy value">${ic("copy")}</button>
        <button class="btn icon" data-a="del" title="Delete" aria-label="Delete variable">${ic("trash")}</button>
      </span></div>`;
  }).join("");
  const note = !cur.encrypted && m && m.status === "plain-risk"
    ? `<div class="note warn">${ic("alert")}This file is plaintext and Git isn't ignoring it.</div>`
    : m && m.drift ? `<div class="note warn">${ic("alert")}Recipients differ from the .sops.yaml rule.<button class="btn sm" id="fixDrift">Review</button></div>` : "";
  el.innerHTML = `${note}
    <div class="filehead"><span class="name mono">${esc(m ? m.name : cur.path)}</span>
      <span class="muted small">${cur.encrypted ? `${cur.recipients.length} recipient${cur.recipients.length === 1 ? "" : "s"}` : "Not encrypted yet"}</span></div>
    <div class="table">${rows}<button class="addrow" id="addVar">${ic("plus")}Add variable</button></div>
    <div class="foot"><span class="muted small" id="dirty"></span><span class="spacer"></span>
      <button class="btn" id="revert" hidden>Discard</button>
      <button class="btn primary" id="save">${ic("lock")}Encrypt and save</button></div>`;

  el.querySelectorAll(".row").forEach((row) => {
    const i = +row.dataset.i, it = cur.items[i];
    const tint = () => {
      row.classList.toggle("added", !orig.has(it.k));
      row.classList.toggle("changed", orig.has(it.k) && orig.get(it.k) !== it.v);
      mark();
    };
    row.querySelector(".k").oninput = (e) => { it.k = e.target.value.trim(); tint(); };
    row.querySelector(".val").oninput = (e) => { it.v = e.target.value; tint(); };
    row.querySelector("[data-a=eye]").onclick = () => { shown.has(i) ? shown.delete(i) : shown.add(i); renderEditor(); };
    row.querySelector("[data-a=copy]").onclick = () => copy(it.v, it.k || "Value");
    row.querySelector("[data-a=del]").onclick = () => { cur.items.splice(i, 1); shown.clear(); renderEditor(); };
  });
  $("#addVar").onclick = () => {
    cur.items.push({ t: "kv", k: "", v: "" });
    shown.add(cur.items.length - 1);
    renderEditor();
    const ks = el.querySelectorAll(".k"); ks[ks.length - 1].focus();
  };
  $("#save").onclick = save;
  $("#revert").onclick = () => { cur.items = JSON.parse(cur.orig); shown.clear(); renderEditor(); };
  if ($("#fixDrift")) $("#fixDrift").onclick = recipientsDialog;
  mark();
}
function mark() {
  const d = isDirty();
  $("#dirty").textContent = d ? "Unsaved changes" : cur.encrypted ? "Saved and encrypted" : "";
  $("#revert").hidden = !d;
}

async function save() {
  if (cur.items.some((i) => i.t === "kv" && !i.k)) return toast("Enter a name for every variable.", true);
  if (!cur.encrypted && !cur.hasRule) return chooseRecipients();
  await doSave();
}
async function doSave(recipients) {
  try {
    const d = await api("save", { path: cur.path, items: cur.items, recipients });
    cur = { path: cur.path, ...d.file, orig: JSON.stringify(d.file.items) };
    setState(d.state); renderEditor();
    toast("Encrypted and saved");
  } catch (e) { fail(e); }
}

// ---- dialogs -------------------------------------------------------------
function show(dlg, title, html) {
  dlg.innerHTML = `<div class="dhead"><span>${title}</span><span class="spacer"></span>
    <button class="btn icon" data-close aria-label="Close">${ic("x")}</button></div><div class="dbody">${html}</div>`;
  dlg.querySelector("[data-close]").onclick = () => dlg.close();
  if (!dlg.open) dlg.showModal();
}
document.querySelectorAll("dialog").forEach((d) => d.addEventListener("click", (e) => { if (e.target === d) d.close(); }));

// Everyone I could encrypt for: my own keys first, then saved recipients.
function candidates() {
  const list = [];
  for (const k of S.keys) for (const p of k.pubs) list.push({ key: p, name: `You (${k.label})`, mine: true });
  for (const b of S.book) if (!list.some((c) => c.key === b.key)) list.push({ key: b.key, name: b.name });
  return list;
}

function chooseRecipients() {
  const c = candidates(), dlg = $("#dlg");
  if (!c.length) { toast("Add or create a key first.", true); return keysDialog(); }
  show(dlg, "Encrypt for", `<p class="muted small">Everyone you tick can decrypt this file with their own key. Keep yourself ticked, or you can't open it again.</p>
    ${c.map((r, i) => `<label class="check"><input type="checkbox" data-i="${i}" ${r.mine ? "checked" : ""}>
      <span>${esc(r.name)}</span><span class="mono muted small">${short(r.key)}</span></label>`).join("")}
    <div class="formrow" style="justify-content:flex-end"><button class="btn primary" id="go">${ic("lock")}Encrypt and save</button></div>`);
  $("#go", dlg).onclick = async () => {
    const picked = [...dlg.querySelectorAll("input:checked")].map((x) => c[x.dataset.i].key);
    if (!picked.length) return toast("Tick at least one recipient.", true);
    dlg.close(); await doSave(picked);
  };
}

function keysDialog() {
  const dlg = $("#dlg");
  const kind = { default: "Default location", path: "Custom path", session: "Session only" };
  const items = S.keys.map((k) => {
    const n = S.files.filter((f) => (f.recipients || []).some((r) => k.pubs.includes(r))).length;
    return `<div class="item"><div class="top"><strong>${esc(k.label)}</strong>
        <span class="pill ${k.kind === "session" ? "warn" : ""}">${kind[k.kind]}</span><span class="spacer"></span>
        ${k.pubs.length ? `<span class="muted small">opens ${n} file${n === 1 ? "" : "s"}</span>` : ""}
        ${k.kind === "session" ? `<button class="btn sm" data-persist="${esc(k.id)}">Save to default location</button>` : ""}
        ${k.kind !== "default" ? `<button class="btn icon" data-rm="${esc(k.id)}" title="Remove from this app" aria-label="Remove key">${ic("x")}</button>` : ""}</div>
      ${k.path ? `<div class="sub"><span class="mono">${esc(k.path)}</span></div>` : ""}
      ${k.error ? `<div class="err">${esc(k.error)}</div>` : ""}
      ${k.pubs.map((p) => `<div class="sub"><span class="mono">${esc(p)}</span>
        <button class="btn icon" data-copy="${esc(p)}" title="Copy public key" aria-label="Copy public key">${ic("copy")}</button></div>`).join("")}</div>`;
  }).join("");
  show(dlg, "Your keys", `${items}
    <div class="formrow">
      <button class="btn" data-f="path">${ic("folder")}Add key file</button>
      <button class="btn" data-f="paste">${ic("copy")}Paste keys.txt</button>
      <button class="btn" data-f="new">${ic("plus")}Create key</button></div>
    <div id="kform"></div>
    <p class="muted small">The public key (age1…) is safe to share. Secret keys never leave this machine.</p>`);
  dlg.querySelectorAll("[data-copy]").forEach((b) => (b.onclick = () => copy(b.dataset.copy, "Public key")));
  dlg.querySelectorAll("[data-rm]").forEach((b) => (b.onclick = () => act("removeKey", { id: b.dataset.rm }, keysDialog)));
  dlg.querySelectorAll("[data-persist]").forEach((b) => (b.onclick = async () =>
    (await ask(`Save this key to\n${S.defaultKeyPath}?\n\nIt's added to the file; existing keys are kept.`, "Save key")) &&
    act("persistKey", { id: b.dataset.persist }, keysDialog, "Key saved")));
  const form = $("#kform", dlg);
  dlg.querySelector("[data-f=path]").onclick = async () => {
    const p = await pick("file", "Choose a key file");
    if (p) act("addKeyPath", { path: p }, keysDialog, "Key file added");
  };
  dlg.querySelector("[data-f=paste]").onclick = () => {
    form.innerHTML = `<div class="panel"><textarea id="kc" rows="3" class="mono" placeholder="AGE-SECRET-KEY-1…" spellcheck="false" aria-label="Key content"></textarea>
      <div class="formrow"><input type="text" id="kl" placeholder="Label, like homelab" aria-label="Label">
      <button class="btn" id="ks">Use for this session</button><button class="btn primary" id="kp">Save to default location</button></div></div>`;
    $("#kc", form).focus();
    const add = (persist) => async () => {
      try {
        const d = await api("addSessionKey", { content: $("#kc", form).value, label: $("#kl", form).value });
        const s = persist ? await api("persistKey", { id: d.id }) : d.state;
        await refreshAfter(s); keysDialog(); toast(persist ? "Key saved" : "Key loaded for this session");
      } catch (e) { fail(e); }
    };
    $("#ks", form).onclick = add(false); $("#kp", form).onclick = add(true);
  };
  dlg.querySelector("[data-f=new]").onclick = () => {
    form.innerHTML = `<div class="panel"><div class="muted small">A new age key pair is created on this machine.</div>
      <div class="formrow"><input type="text" id="kl" placeholder="Label, like work" aria-label="Label">
      <button class="btn" id="ks">Keep for this session</button><button class="btn primary" id="kp">Save to default location</button></div></div>`;
    const gen = (persist) => async () => {
      try {
        const d = await api("generateKey", { persist, label: $("#kl", form).value });
        await refreshAfter(d.state); keysDialog(); toast("Key created");
      } catch (e) { fail(e); }
    };
    $("#ks", form).onclick = gen(false); $("#kp", form).onclick = gen(true);
  };
}

// After keys or recipients change, the open file may have become (un)readable.
async function refreshAfter(s) {
  setState(s);
  if (cur && !isDirty()) { const f = meta(); cur = null; await openFile(f); }
  else renderEditor();
}
async function act(action, body, again, msg) {
  try { await refreshAfter(await api(action, body)); if (msg) toast(msg); again && again(); } catch (e) { fail(e); }
}

function recipientsDialog() {
  const dlg = $("#dlg"), m = meta();
  let fileHtml = "";
  if (cur && cur.encrypted) {
    const have = cur.recipients.map((r) => r.key);
    const addable = candidates().filter((c) => !have.includes(c.key));
    const nameOf = (k) => (candidates().find((c) => c.key === k) || {}).name || short(k);
    const drift = m && m.drift ? `<div class="note warn" style="margin:8px 0 0">${ic("alert")}<span>.sops.yaml expects
      ${m.drift.missing.length ? "also " + m.drift.missing.map(nameOf).map(esc).join(", ") : ""}${m.drift.missing.length && m.drift.extra.length ? "; " : ""}${m.drift.extra.length ? "not " + m.drift.extra.map(nameOf).map(esc).join(", ") : ""}.</span>
      <button class="btn sm" id="apply">Update file</button></div>` : "";
    fileHtml = `<h3>Who can decrypt ${esc(m ? m.name : "")}</h3>
      <div class="chips">${cur.recipients.map((r) => `<span class="chip ${r.mine ? "mine" : ""}" title="${esc(r.key)}">${esc(r.name)} <span class="mono" style="opacity:.7;font-size:11.5px">…${esc(r.key.slice(-4))}</span>
        ${r.mine ? "" : `<button data-drop="${esc(r.key)}" aria-label="Remove ${esc(r.name)}">${ic("x")}</button>`}</span>`).join("")}</div>
      ${addable.length ? `<div class="formrow"><select id="addSel" aria-label="Recipient to add" style="flex:1">${addable.map((c) =>
        `<option value="${esc(c.key)}">${esc(c.name)} · ${short(c.key)}</option>`).join("")}</select><button class="btn" id="addRec">Add to file</button></div>` : ""}
      ${drift}`;
  }
  const book = S.book.map((b) => `<div class="item"><div class="top"><strong>${esc(b.name)}</strong><span class="spacer"></span>
      <button class="btn icon" data-rm="${esc(b.key)}" title="Remove" aria-label="Remove ${esc(b.name)}">${ic("x")}</button></div>
      <div class="sub"><span class="mono">${esc(b.key)}</span></div></div>`).join("");
  show(dlg, "Recipients", `${fileHtml}
    <h3>Saved recipients</h3>
    ${book || `<div class="muted small" style="padding:6px 0">Save a teammate's or server's public key here to pick it by name.</div>`}
    <div class="formrow"><input type="text" id="bn" placeholder="Name" style="flex:0 1 130px" aria-label="Name">
      <input type="text" id="bk" class="mono" placeholder="age1…" spellcheck="false" aria-label="Public key"><button class="btn" id="badd">Save</button></div>`);
  const setRec = async (list, msg) => {
    if (isDirty()) return toast("Save or discard your changes first.", true);
    try {
      const d = await api("setRecipients", { path: cur.path, recipients: list });
      cur = { path: cur.path, ...d.file, orig: JSON.stringify(d.file.items) };
      setState(d.state); renderEditor(); recipientsDialog(); toast(msg);
    } catch (e) { fail(e); }
  };
  dlg.querySelectorAll("[data-drop]").forEach((b) => (b.onclick = async () =>
    (await ask("Remove this recipient?\n\nThey can no longer decrypt new versions, but may have seen the current values. Change those secrets if that matters.", "Remove")) &&
    setRec(cur.recipients.map((r) => r.key).filter((k) => k !== b.dataset.drop), "Recipient removed")));
  if ($("#addRec", dlg)) $("#addRec", dlg).onclick = () =>
    setRec([...cur.recipients.map((r) => r.key), $("#addSel", dlg).value], "Recipient added");
  if ($("#apply", dlg)) $("#apply", dlg).onclick = () => act("applyRule", { path: cur.path }, recipientsDialog, "File updated");
  dlg.querySelectorAll("[data-rm]").forEach((b) => (b.onclick = () => act("bookRemove", { key: b.dataset.rm }, recipientsDialog)));
  $("#badd", dlg).onclick = () => act("bookAdd", { name: $("#bn", dlg).value, key: $("#bk", dlg).value }, recipientsDialog, "Recipient saved");
}

// Native file and folder dialogs. Returns the chosen path, or null.
async function pick(mode, title, start) {
  try {
    return await T.dialog.open({ directory: mode === "folder", multiple: false, title, defaultPath: start || undefined });
  } catch { toast("Couldn't open the file dialog.", true); return null; }
}

// In-app confirmation. Resolves true only when the action button is pressed.
function ask(message, okLabel) {
  return new Promise((resolve) => {
    const dlg = $("#dlg2");
    let answer = false;
    show(dlg, "Confirm", `<p style="white-space:pre-line;overflow-wrap:anywhere">${esc(message)}</p>
      <div class="formrow" style="justify-content:flex-end"><button class="btn" id="no">Cancel</button>
      <button class="btn primary" id="yes">${esc(okLabel)}</button></div>`);
    $("#yes", dlg).onclick = () => { answer = true; dlg.close(); };
    $("#no", dlg).onclick = () => dlg.close();
    dlg.addEventListener("close", () => resolve(answer), { once: true });
  });
}

$("#keysBtn").onclick = keysDialog;
$("#recBtn").onclick = recipientsDialog;
$("#folderBtn").onclick = async () => {
  if (isDirty() && !(await ask("Discard unsaved changes?", "Discard"))) return;
  const p = await pick("folder", "Choose a project folder", S.folder);
  if (!p) return;
  try { cur = null; setState(await api("setFolder", { path: p })); renderEditor(); } catch (e) { fail(e); }
};

// Ask before closing the window with unsaved edits.
try {
  T.window.getCurrentWindow().onCloseRequested(async (event) => {
    if (isDirty() && !(await ask("Close and discard unsaved changes?", "Discard"))) event.preventDefault();
  });
} catch { /* closing simply won't ask */ }

api("state").then((s) => { setState(s); renderEditor(); }).catch(fail);
