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
  refresh: '<path d="M20 11a8 8 0 1 0-2.3 5.7M20 4v7h-7"/>',
  upgrade: '<circle cx="12" cy="12" r="9"/><path d="M12 16.5v-9M8 11l4-4 4 4"/>',
  gear: '<path d="M4 7h9M19 7h1M4 17h1M11 17h9"/><circle cx="16" cy="7" r="2.5"/><circle cx="8" cy="17" r="2.5"/>',
  branch: '<circle cx="6" cy="5" r="2.5"/><circle cx="6" cy="19" r="2.5"/><circle cx="18" cy="8" r="2.5"/><path d="M6 7.5v9M18 10.5c0 5-12 2-12 6"/>',
  up: '<path d="M12 19V5M6 11l6-6 6 6"/>',
  alert: '<path d="M12 3l9.5 17h-19z"/><path d="M12 10v4M12 17.5v.1"/>',
  expand: '<path d="M14 4h6v6M10 20H4v-6M20 4l-7 7M4 20l7-7"/>',
  shrink: '<path d="M20 4l-7 7M13 5v6h6M4 20l7-7M11 19v-6H5"/>',
};
const ic = (n) => `<svg viewBox="0 0 24 24" aria-hidden="true">${ICONS[n]}</svg>`;
const esc = (s) => String(s).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));
const short = (k) => k.slice(0, 10) + "…" + k.slice(-6);
document.querySelectorAll("[data-ic]").forEach((e) => (e.innerHTML = ic(e.dataset.ic)));

let S = null;          // server state
// Open file: { path, format, kind: "env" | "text", items + orig or text + origText, recipients, rev }
let cur = null;
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
// Copying goes through the backend, which also removes a secret from the
// clipboard again after the time chosen in the settings.
const duration = (s) => (s % 60 === 0 ? `${s / 60} min` : `${s} s`);
async function copy(text, what, secret = false) {
  try {
    const seconds = await T.core.invoke("copy", { text, secret });
    toast(seconds ? `${what} copied. The clipboard is cleared in ${duration(seconds)}.` : `${what} copied`);
  } catch (e) { toast(typeof e === "string" ? e : "Couldn't copy to the clipboard.", true); }
}

// ---- theme ---------------------------------------------------------------
const media = matchMedia("(prefers-color-scheme: dark)");
function theme() { try { return localStorage.getItem("theme") || "system"; } catch { return "system"; } }
function applyTheme() {
  const t = theme();
  document.documentElement.dataset.theme = t === "system" ? (media.matches ? "dark" : "light") : t;
}
function setTheme(name) {
  try { localStorage.setItem("theme", name); } catch { /* applies until the app closes */ }
  applyTheme();
}
media.addEventListener("change", applyTheme);
applyTheme();

// ---- file list width -------------------------------------------------------
// The file list's right edge can be dragged (or moved with the arrow keys
// while focused); double-click resets it. The width is a per-machine
// convenience, so it lives in localStorage like the theme.
const SIDE_DEFAULT = 250, SIDE_MIN = 160, SIDE_MAX = 600;
// Leave the editor room to work, however narrow the window gets.
const sideMax = () => Math.max(SIDE_MIN, Math.min(SIDE_MAX, innerWidth - 360));
const clampSide = (w) => Math.round(Math.min(sideMax(), Math.max(SIDE_MIN, w)));
function sideWidth() {
  try { return Number(localStorage.getItem("sideWidth")) || SIDE_DEFAULT; } catch { return SIDE_DEFAULT; }
}
function applySideWidth(w = sideWidth()) {
  const px = clampSide(w), r = $("#resizer");
  document.documentElement.style.setProperty("--side-w", `${px}px`);
  r.setAttribute("aria-valuemin", SIDE_MIN);
  r.setAttribute("aria-valuemax", sideMax());
  r.setAttribute("aria-valuenow", px);
  return px;
}
function setSideWidth(w) {
  const px = applySideWidth(w);
  try {
    if (px === SIDE_DEFAULT) localStorage.removeItem("sideWidth");
    else localStorage.setItem("sideWidth", String(px));
  } catch { /* applies until the app closes */ }
}
{
  const r = $("#resizer");
  let start = null; // { x, w } while dragging
  r.addEventListener("pointerdown", (e) => {
    if (e.button !== 0) return;
    e.preventDefault();
    start = { x: e.clientX, w: $("aside").getBoundingClientRect().width };
    try { r.setPointerCapture(e.pointerId); } catch { /* the window listeners below still follow */ }
    document.body.classList.add("resizing");
  });
  window.addEventListener("pointermove", (e) => {
    if (start) applySideWidth(start.w + e.clientX - start.x);
  });
  const end = () => {
    if (!start) return;
    start = null;
    document.body.classList.remove("resizing");
    setSideWidth(parseFloat(getComputedStyle(document.documentElement).getPropertyValue("--side-w")));
  };
  window.addEventListener("pointerup", end);
  window.addEventListener("pointercancel", end);
  r.addEventListener("dblclick", () => setSideWidth(SIDE_DEFAULT));
  r.addEventListener("keydown", (e) => {
    const step = e.shiftKey ? 64 : 16, now = $("aside").getBoundingClientRect().width;
    const to = { ArrowLeft: now - step, ArrowRight: now + step, Home: SIDE_MIN, End: sideMax() }[e.key];
    if (to === undefined) return;
    e.preventDefault();
    setSideWidth(to);
  });
  // A smaller window narrows the list for now; the saved width comes back when there's room.
  window.addEventListener("resize", () => applySideWidth());
  applySideWidth();
}

// ---- main view -----------------------------------------------------------
function setState(s) {
  S = s;
  // The button truncates on the left (direction: rtl); <bdi> keeps the path itself left-to-right.
  $("#folderBtn").innerHTML = `<bdi>${esc(S.folder)}</bdi>`;
  const hasKey = S.keys.some((k) => k.pubs.length);
  $("#keysDot").className = hasKey ? "" : "dot";
  const banner = $("#banner");
  banner.style.display = S.sops.path && S.sops.supported ? "none" : "block";
  const brew = S.os === "darwin" ? ` (<span class="mono">brew install sops</span>)` : "";
  const from = `from <span class="mono">github.com/getsops/sops/releases</span>`;
  banner.innerHTML = S.sops.path
    ? `sops ${esc(S.sops.version)} is too old; saving may fail. Install sops 3.11 or newer${brew} ${from}, then restart this app.`
    : `sops wasn't found. Install it${brew} ${from}` +
      (S.appDir ? `, or put the sops binary in <span class="mono">${esc(S.appDir)}</span>` : "") + `. Then restart this app.`;
  renderFiles();
  renderUpdate();
}
function isDirty() {
  if (!cur || cur.blocked) return false;
  return cur.kind === "text" ? cur.text !== cur.origText : JSON.stringify(cur.items) !== cur.orig;
}
// The open file as the editor keeps it, from what the backend returned.
// A textarea only knows \n; saving puts the file's own line endings back.
function loaded(path, d) {
  if (d.kind !== "text") return { path, ...d, orig: JSON.stringify(d.items) };
  const text = d.text.replace(/\r\n/g, "\n");
  return { path, ...d, text, origText: text };
}
// Scanning the folder can take a moment in a big project; the file list shows
// a thin bar meanwhile (after a short delay, see .scanbar in style.css).
let scans = 0;
async function scanning(promise) {
  scans++;
  $("aside").classList.add("scanning");
  try { return await promise; } finally { if (--scans === 0) $("aside").classList.remove("scanning"); }
}
function renderFiles() {
  const icon = { ok: "lock", locked: "lock", missing: "file", unreadable: "file" };
  const gitMark = { modified: ["M", "Changed since the last commit"], new: ["N", "Not committed yet"] };
  const button = ({ f, i, base }) => {
    const mark = gitMark[f.git];
    return `<button class="file s-${f.status} ${cur && cur.path === f.path ? "active" : ""}" data-i="${i}" title="${esc(f.name)}\n${esc(f.note)}${f.format ? ` · ${FORMATS[f.format] || f.format}` : ""}">
      ${ic(icon[f.status])}<span class="mono">${esc(base)}</span>${mark ? `<span class="gs gs-${f.git}" title="${mark[1]}">${mark[0]}</span>` : ""}</button>`;
  };
  // Files are grouped under their folder, so long paths don't push the
  // file name out of view. Files directly at the top come first.
  const section = (entries) => {
    const groups = new Map();
    for (const e of entries) {
      const cut = Math.max(e.path.lastIndexOf("/"), e.path.lastIndexOf("\\"));
      const dir = cut < 0 ? "" : e.path.slice(0, cut + 1);
      if (!groups.has(dir)) groups.set(dir, []);
      groups.get(dir).push({ ...e, base: e.path.slice(cut + 1) });
    }
    const dirs = [...groups.keys()].filter((d) => d).sort((a, b) => a.localeCompare(b));
    return (groups.get("") || []).map(button).join("") + dirs.map((d) => {
      // A subfolder listed right under its parent shows only the rest of its path, indented.
      const parent = dirs.filter((p) => p !== d && d.startsWith(p)).sort((x, y) => y.length - x.length)[0];
      const label = parent ? d.slice(parent.length) : d;
      return `<div class="dir mono${parent ? " sub" : ""}" title="${esc(d)}"><bdi>${esc(label)}</bdi></div>` +
        groups.get(d).map(button).join("");
    }).join("");
  };
  // Git repositories are the usual home of env files: each gets its own
  // section with its branch. Files outside any repository come first, and a
  // folder without repositories looks exactly as it would without Git.
  const all = S.files.map((f, i) => ({ f, i }));
  const loose = all.filter((e) => !e.f.repo).map((e) => ({ ...e, path: e.f.name }));
  const repos = (S.repos || []).map((r) => `<div class="repo" title="${esc(r.root)}">${ic("branch")}<span class="rname">${esc(r.name || r.root)}</span>` +
    (r.branch ? `<span class="branch mono" title="Current branch">${esc(r.branch)}</span>` : "") + `</div>` +
    section(all.filter((e) => e.f.repo === r.root).map((e) => ({ ...e, path: e.f.rel || e.f.name }))));
  $("#files").innerHTML = section(loose) + repos.join("") +
    (S.files.length ? "" : `<div class="muted small" style="padding:6px 8px">No SOPS-encrypted files in this folder.</div>`) +
    (S.scanLimited ? `<div class="small limitnote">This folder is too big to search completely. Open a subfolder, or lower the search depth in the settings.</div>` : "") +
    `<button class="file addfile" id="newFile">${ic("plus")}<span>New file</span></button>` +
    `<button class="file addfile" id="addFile">${ic("file")}<span>Open file</span></button>` +
    (S.sops.path ? `<div class="sopsinfo" title="${esc(S.sops.path)}">sops ${esc(S.sops.version)} · ${S.sops.source}</div>` : "");
  $("#files").querySelectorAll("[data-i]").forEach((b) => (b.onclick = () => openFile(S.files[b.dataset.i])));
  $("#newFile").onclick = newFileDialog;
  $("#addFile").onclick = async () => {
    const p = await pick("file", "Open SOPS file", S.folder);
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
  if (f.status === "locked" || f.status === "missing" || f.status === "unreadable") { cur = { path: f.path, blocked: f }; renderFiles(); return renderEditor(); }
  try {
    cur = loaded(f.path, await api("open", { path: f.path }));
  } catch (e) { cur = null; fail(e); }
  renderFiles(); renderEditor();
}
function meta() { return S.files.find((f) => cur && f.path === cur.path); }

const FORMATS = { dotenv: "Env", yaml: "YAML", json: "JSON", ini: "INI", binary: "Text" };
const HIGHLIGHT_MAX = 300_000;

function renderEditor() {
  const el = $("#editor");
  if (!cur) {
    el.innerHTML = `<div class="empty">${ic("lock")}<h2>Open a SOPS file</h2>
      <p>Pick a file on the left to view and edit it. Files are decrypted in memory only.</p>
      <p><button class="btn" id="emptyNew">${ic("plus")}New encrypted file</button></p></div>`;
    $("#emptyNew").onclick = newFileDialog;
    return;
  }
  const m = meta() || cur.blocked;
  if (cur.blocked) {
    const want = (m.recipients || []).map((k) => `<div class="mono small">${esc(k)}</div>`).join("");
    if (m.status === "unreadable") {
      el.innerHTML = `<div class="empty">${ic("file")}<h2>This file can't be shown</h2><p>${esc(m.note)}.</p></div>`;
      return;
    }
    el.innerHTML = `<div class="empty">${ic("key")}<h2>${m.status === "missing" ? "File not found" : "No matching key"}</h2>
      ${m.status === "missing" ? "" : `<p>None of your loaded keys can open <span class="mono">${esc(m.name)}</span>. It's encrypted for:</p>${want}
      <p><button class="btn" id="goKeys">${ic("key")}Add a key</button></p>`}</div>`;
    if ($("#goKeys")) $("#goKeys").onclick = keysDialog;
    return;
  }
  const text = cur.kind === "text";
  // A text file in a binary wrapper is saved in place; YAML, JSON and INI are encrypted again.
  const cantSave = text && !cur.isNew && cur.reencrypt && cur.format !== "binary";
  let note = "";
  if (cur.gone) {
    note = `<div class="note warn">${ic("alert")}<span>This file is no longer on disk, for example after switching branches.${cur.isNew || text ? "" : " Saving creates it again."}</span></div>`;
  } else if (cur.stale) {
    note = `<div class="note warn">${ic("alert")}<span>This file changed on disk while you were editing. Your edits are kept; saving asks whether to overwrite the other version.</span></div>`;
  } else if (cantSave) {
    note = `<div class="note warn">${ic("alert")}<span>${esc(cur.reencrypt)}</span></div>`;
  } else if (m && m.drift) {
    note = `<div class="note warn">${ic("alert")}<span>Recipients differ from the .sops.yaml rule.</span><button class="btn sm" id="fixDrift">Review</button></div>`;
  }
  let body;
  if (text) {
    // Coloured syntax sits behind a transparent textarea, so typing, selecting
    // and undo stay the browser's own. Masked or plain text needs no layer.
    body = `<div class="codewrap${cur.revealed ? "" : " masked"}" id="codeWrap"><pre class="hl" aria-hidden="true"><code></code></pre>
      <textarea class="textedit" id="textEdit" spellcheck="false" autocomplete="off" autocapitalize="off" wrap="off"
      aria-label="File content" placeholder="${cur.isNew ? "Content of the new file" : ""}"></textarea></div>`;
  } else {
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
    body = `<div class="table">${rows}<button class="addrow" id="addVar">${ic("plus")}Add variable</button></div>`;
  }
  // Comparing with the last commit needs a repository and git itself.
  const canCompare = m && m.repo && S.git && (m.git === "modified" || m.git === "new");
  const count = (cur.recipients || []).length;
  const fmt = FORMATS[cur.format];
  // Env files: one button reveals or hides every value; "Hide all" once each is shown.
  const kvRows = text ? [] : cur.items.map((it, i) => (it.t === "kv" ? i : -1)).filter((i) => i >= 0);
  const allShown = kvRows.length > 0 && kvRows.every((i) => shown.has(i));
  el.innerHTML = `${note}
    <div class="filehead"><span class="name mono">${esc(m ? m.name : cur.isNew ? cur.newName : cur.path)}</span>
      <span class="muted small">${cur.isNew ? "New file, saved encrypted" : `${count} recipient${count === 1 ? "" : "s"}`}</span>
      ${fmt && cur.format !== "dotenv" ? `<span class="pill fmt" title="Edited as text">${fmt}</span>` : ""}
      <span class="tools">
        ${text ? `<button class="btn sm" id="revealText">${ic(cur.revealed ? "eyeoff" : "eye")}${cur.revealed ? "Hide" : "Show"}</button>
          <button class="btn sm" id="copyText" title="Copy the whole content">${ic("copy")}Copy</button>` : ""}
        ${kvRows.length ? `<button class="btn sm" id="revealAll">${ic(allShown ? "eyeoff" : "eye")}${allShown ? "Hide all" : "Show all"}</button>` : ""}
        ${canCompare ? `<button class="btn sm" id="changesBtn" title="Compare with the last commit">${ic("branch")}Changes</button>` : ""}
      </span></div>
    ${body}
    <div class="foot"><span class="muted small" id="dirty"></span><span class="spacer"></span>
      <button class="btn" id="revert" hidden>Discard</button>
      <button class="btn primary" id="save" ${cantSave ? `disabled title="${esc(cur.reencrypt)}"` : ""}>${ic("lock")}Encrypt and save</button></div>`;
  if ($("#changesBtn")) $("#changesBtn").onclick = changesDialog;
  if ($("#revealAll")) $("#revealAll").onclick = () => {
    if (allShown) shown.clear();
    else kvRows.forEach((i) => shown.add(i));
    renderEditor();
  };

  if (text) {
    const area = $("#textEdit");
    // Set as a value, not as markup: the parser would drop a leading line break.
    area.value = cur.text;
    const wrap = $("#codeWrap"), layer = $("#codeWrap .hl"), code = $("#codeWrap code");
    const lang = highlight.language(cur.format, cur.newName || cur.path);
    const paint = () => {
      // Very large files stay plain; colouring them on every key would lag.
      const on = !!lang && cur.revealed && area.value.length <= HIGHLIGHT_MAX;
      wrap.classList.toggle("coloured", on);
      if (!on) return;
      code.innerHTML = highlight.html(lang, area.value) + "\n"; // the extra line keeps a final empty line in view
      layer.scrollTop = area.scrollTop;
      layer.scrollLeft = area.scrollLeft;
    };
    area.onscroll = () => { layer.scrollTop = area.scrollTop; layer.scrollLeft = area.scrollLeft; };
    area.oninput = () => { cur.text = area.value; paint(); mark(); };
    paint();
    $("#revealText").onclick = () => { cur.revealed = !cur.revealed; renderEditor(); };
    $("#copyText").onclick = () => copy(cur.text, "Content", true);
  } else {
    const orig = new Map(JSON.parse(cur.orig).filter((i) => i.t === "kv").map((i) => [i.k, i.v]));
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
      row.querySelector("[data-a=copy]").onclick = () => copy(it.v, it.k || "Value", true);
      row.querySelector("[data-a=del]").onclick = () => { cur.items.splice(i, 1); shown.clear(); renderEditor(); };
    });
    $("#addVar").onclick = () => {
      cur.items.push({ t: "kv", k: "", v: "" });
      shown.add(cur.items.length - 1);
      renderEditor();
      const ks = el.querySelectorAll(".k"); ks[ks.length - 1].focus();
    };
  }
  $("#save").onclick = save;
  $("#revert").onclick = async () => {
    // After an outside change, discarding means taking the version on disk.
    if (cur.stale && meta()) { const f = meta(); cur = null; return openFile(f); }
    if (text) cur.text = cur.origText;
    else cur.items = JSON.parse(cur.orig);
    shown.clear(); renderEditor();
  };
  if ($("#fixDrift")) $("#fixDrift").onclick = recipientsDialog;
  mark();
}
function mark() {
  const d = isDirty();
  $("#dirty").textContent = d ? "Unsaved changes" : cur.isNew ? "" : "Saved and encrypted";
  $("#revert").hidden = !d;
}

async function save() {
  if (cur.kind !== "text" && cur.items.some((i) => i.t === "kv" && !i.k)) return toast("Enter a name for every variable.", true);
  if (cur.isNew && !cur.hasRule) return chooseRecipients();
  await doSave();
}
let saving = false;
async function doSave(recipients, overwrite = false) {
  saving = true;
  try {
    const content = cur.kind === "text" ? { text: cur.text } : { items: cur.items };
    if (cur.isNew) {
      // Created encrypted in one step; the plaintext never reaches the disk.
      const d = await api("create", { dir: cur.newDir, name: cur.newName, ...content, recipients });
      cur = { ...loaded(d.path, d.file), revealed: cur.revealed };
      setState(d.state); renderEditor();
      toast("Created and encrypted");
      return;
    }
    const d = await api("save", { path: cur.path, ...content, rev: cur.rev, overwrite });
    if (d.conflict) return conflictDialog(recipients);
    cur = { ...loaded(cur.path, d.file), revealed: cur.revealed };
    setState(d.state); renderEditor();
    toast("Encrypted and saved");
  } catch (e) { fail(e); }
  finally { saving = false; }
}

// A new encrypted file: pick a folder and a name, then fill in the content in
// the editor. The name sets the format. Saving creates the file, already encrypted.
async function newFileDialog() {
  if (isDirty() && !(await ask("Discard unsaved changes?", "Discard"))) return;
  const sep = S.folder.includes("\\") ? "\\" : "/";
  const dirOf = (p) => p.slice(0, Math.max(p.lastIndexOf("/"), p.lastIndexOf("\\")));
  // Likely homes: the open folder, each repository, and folders that already hold encrypted files.
  const dirs = [...new Set([S.folder, ...(S.repos || []).map((r) => r.root), ...S.files.map((f) => dirOf(f.path))])];
  const label = (d) => (d === S.folder ? `${d.split(/[\\/]/).pop() || d} (open folder)` : d.startsWith(S.folder + sep) ? d.slice(S.folder.length + 1) : d);
  const dlg = $("#dlg");
  show(dlg, "New encrypted file", `
    <div class="formrow" style="align-items:center"><label for="nfDir" style="width:70px">Folder</label>
      <select id="nfDir" style="flex:1">${dirs.map((d) => `<option value="${esc(d)}">${esc(label(d))}</option>`).join("")}
        <option value="">Choose another folder…</option></select></div>
    <div class="formrow" style="align-items:center"><label for="nfName" style="width:70px">Name</label>
      <input type="text" id="nfName" class="mono" placeholder=".env.production" spellcheck="false"></div>
    <div class="err" id="nfErr"></div>
    <p class="muted small">The name sets the format: env files get a table of variables; .yaml, .json, .ini and any other name a text editor.
      The file is created encrypted when you save it; its content never touches the disk in plaintext.</p>
    <div class="formrow" style="justify-content:flex-end"><button class="btn primary" id="nfGo">Continue</button></div>`);
  const sel = $("#nfDir", dlg), name = $("#nfName", dlg), err = $("#nfErr", dlg);
  sel.onchange = async () => {
    if (sel.value) return;
    const p = await pick("folder", "Choose a folder for the new file", S.folder);
    if (!p) { sel.selectedIndex = 0; return; }
    sel.insertBefore(Object.assign(document.createElement("option"), { value: p, textContent: p }), sel.lastElementChild);
    sel.value = p;
  };
  name.oninput = () => (err.textContent = "");
  name.onkeydown = (e) => { if (e.key === "Enter") $("#nfGo", dlg).click(); };
  name.focus();
  $("#nfGo", dlg).onclick = async () => {
    try {
      const d = await api("checkNewFile", { dir: sel.value, name: name.value });
      dlg.close();
      const base = { path: d.path, isNew: true, newDir: sel.value, newName: name.value.trim(), hasRule: d.hasRule,
        format: d.format, kind: d.kind, recipients: [] };
      cur = d.kind === "text" ? { ...base, text: "", origText: "", revealed: true }
        : { ...base, items: [{ t: "kv", k: "", v: "" }], orig: "[]" };
      shown.clear(); shown.add(0);
      renderFiles(); renderEditor();
      (d.kind === "text" ? $("#textEdit") : $("#editor .k")).focus();
    } catch (e) { err.textContent = e.message; }
  };
}

// The file changed on disk after it was opened here, for example through a
// git pull. Nothing has been written; the user decides which version wins.
function conflictDialog(recipients) {
  const dlg = $("#dlg2"), m = meta();
  show(dlg, "This file changed on disk", `
    <p><span class="mono">${esc(m ? m.rel || m.name : cur.path)}</span> was changed outside this app after you opened it, for example by a Git pull or another editor.</p>
    <p class="muted small">Overwriting replaces that version with what you see here. Reloading shows the version on disk and drops your edits.</p>
    <div class="formrow" style="justify-content:flex-end"><button class="btn" id="cfCancel">Cancel</button>
      <button class="btn" id="cfReload">Reload from disk</button><button class="btn primary" id="cfOverwrite">Overwrite</button></div>`);
  $("#cfCancel", dlg).onclick = () => dlg.close();
  $("#cfOverwrite", dlg).onclick = () => { dlg.close(); doSave(recipients, true); };
  $("#cfReload", dlg).onclick = async () => {
    dlg.close();
    const f = meta();
    cur = null;
    await openFile(f);
    toast("Reloaded from disk");
  };
}

// ---- update notice ---------------------------------------------------------
// Asks GitHub for the release list on start and once a day while the app is
// open. The backend decides what is worth announcing (pick_update): stable
// installs hear only about stable releases, pre-releases also about newer
// pre-releases. Nothing is downloaded; the notice links to the release page.
let UPDATE = null;
const DAY = 24 * 60 * 60 * 1000;
// A network that takes the request but never answers mustn't leave a check
// hanging: after this long it counts as failed.
const UPDATE_TIMEOUT_MS = 10_000;

async function releaseFeed() {
  // The UI self-test supplies a fixed list instead of asking GitHub, or asks
  // for a failure ({simulate: "error" | "hang"}) to test what follows.
  const fixture = await T.core.invoke("ui_test", { action: "releases", body: {} }).catch(() => null);
  const ctl = new AbortController();
  const timer = setTimeout(() => ctl.abort(), fixture?.timeoutMs ?? UPDATE_TIMEOUT_MS);
  try {
    if (fixture?.simulate === "error") throw new Error("simulated network error");
    if (fixture?.simulate === "hang") {
      await new Promise((_, reject) => ctl.signal.addEventListener("abort", () => reject(new Error("timed out"))));
    }
    if (fixture) return fixture;
    const res = await fetch("https://api.github.com/repos/nicostoe/open-sops-ui/releases?per_page=30", {
      headers: { Accept: "application/vnd.github+json" },
      signal: ctl.signal,
    });
    if (!res.ok) throw new Error(`GitHub answered ${res.status}`);
    return await res.json(); // also under the time limit: a slow body counts too
  } finally {
    clearTimeout(timer);
  }
}

// One check at a time; asking again while one runs waits for that one.
let checking = null;
function checkForUpdate(manual = false) {
  if (!manual && !S.settings.checkUpdates) return Promise.resolve();
  if (checking) return checking.then(() => manual && reportCheck());
  checking = (async () => {
    try {
      UPDATE = (await api("pickUpdate", { releases: await releaseFeed() })).update;
      lastCheckFailed = false;
      renderUpdate();
    } catch {
      // Offline, rate-limited, timed out or a strange answer. The app keeps
      // working, a notice already shown stays, and nothing is said unless asked.
      lastCheckFailed = true;
    } finally {
      checking = null;
    }
    if (manual) reportCheck();
  })();
  return checking;
}
let lastCheckFailed = false;
function reportCheck() {
  if (lastCheckFailed) toast("Couldn't reach GitHub to check for updates. Try again later.", true);
  else toast(UPDATE ? `Version ${UPDATE.version} is available` : "You have the newest version");
}

function renderUpdate() {
  const b = $("#updateBtn"), on = !!UPDATE && S.settings.checkUpdates;
  b.hidden = !on;
  if (on) b.title = b.ariaLabel = `Version ${UPDATE.version} is available`;
}

// Release notes are Markdown; show a safe subset: headings, list items, plain text.
function notesHtml(md) {
  const inline = (t) => esc(t.replace(/\[([^\]]*)\]\([^)]*\)/g, "$1").replace(/\*\*/g, "").replace(/`/g, "")).trim();
  let html = "", inList = false;
  for (const raw of md.split(/\r?\n/).slice(0, 80)) {
    const line = raw.trim();
    const item = line.match(/^[-*] (.*)/);
    if (item) { if (!inList) { html += "<ul>"; inList = true; } html += `<li>${inline(item[1])}</li>`; continue; }
    if (inList) { html += "</ul>"; inList = false; }
    const head = line.match(/^#{1,6} (.*)/);
    if (head) html += `<h4>${inline(head[1])}</h4>`;
    else if (line) html += `<p class="small">${inline(line)}</p>`;
  }
  return html + (inList ? "</ul>" : "");
}

function updateDialog() {
  if (!UPDATE) return;
  const dlg = $("#dlg"), u = UPDATE;
  show(dlg, "Update available", `
    <p>Open SOPS UI <strong>${esc(u.version)}</strong> is available. You have ${esc(S.version)}.</p>
    ${u.prerelease ? `<p class="muted small">This is a pre-release.</p>` : ""}
    ${u.notes.trim() ? `<div class="notes">${notesHtml(u.notes)}</div>` : ""}
    <div class="formrow" style="justify-content:flex-end"><button class="btn" id="updLater">Not now</button>
      <button class="btn primary" id="updOpen">${ic("upgrade")}Open release page</button></div>`);
  $("#updLater", dlg).onclick = () => dlg.close();
  $("#updOpen", dlg).onclick = async () => {
    try { await T.core.invoke("open_release_page", { tag: u.tag }); dlg.close(); }
    catch (e) { toast(typeof e === "string" ? e : "Couldn't open the browser.", true); }
  };
}

const sameList = (a, b) => a.length === b.length && a.every((x, i) => x === b[i]);
function settingsDialog() {
  const dlg = $("#dlg"), now = S.settings.clipboardClearSeconds;
  const choices = [15, 30, 60, 120, 300, 0];
  if (!choices.includes(now)) choices.unshift(now);
  const t = theme();
  const themes = { system: "Same as the system", light: "Light", dark: "Dark" };
  show(dlg, "Settings", `
    <h3>Appearance</h3>
    <div class="formrow" style="align-items:center"><label for="themeSel" style="flex:1">Theme</label>
      <select id="themeSel">${Object.entries(themes).map(([v, l]) => `<option value="${v}" ${v === t ? "selected" : ""}>${l}</option>`).join("")}</select></div>
    <h3>Clipboard</h3>
    <div class="formrow" style="align-items:center"><label for="clipSel" style="flex:1">Clear copied values after</label>
      <select id="clipSel">${choices.map((s) => `<option value="${s}" ${s === now ? "selected" : ""}>${s ? duration(s) : "Never"}</option>`).join("")}</select></div>
    <p class="muted small">Applies to variable values, not to public keys. Clipboard history tools may keep their own copy.</p>
    <h3>Files</h3>
    <div class="formrow" style="align-items:center"><label for="depthSel" style="flex:1">Search subfolders up to</label>
      <select id="depthSel">${Array.from({ length: 10 }, (_, i) => i + 1).map((n) =>
        `<option value="${n}" ${n === S.settings.scanDepth ? "selected" : ""}>${n} level${n === 1 ? "" : "s"} deep</option>`).join("")}</select></div>
    <label for="skipIn" class="small" style="display:block;margin-top:10px">Skipped folders</label>
    <div class="formrow" style="margin-top:4px"><input type="text" id="skipIn" class="mono" spellcheck="false"
      value="${esc(S.settings.skipDirs.join(", "))}" placeholder="node_modules, vendor">
      <button class="btn sm" id="skipReset" style="height:32px" ${sameList(S.settings.skipDirs, S.settings.defaultSkipDirs) ? "disabled" : ""}>Reset</button></div>
    <p class="muted small">Folder names, separated by commas. Hidden folders such as .git are always skipped.</p>
    <h3>Updates</h3>
    <label class="check setting"><input type="checkbox" id="updChk" ${S.settings.checkUpdates ? "checked" : ""}>
      <span>Look for a new version on start and once a day</span></label>
    <div class="formrow" style="align-items:center"><span class="muted small" style="flex:1">Asks GitHub for the list of releases. Nothing is downloaded or installed.</span>
      <button class="btn sm" id="updNow">Check now</button></div>
    <div class="about small">
      <button class="linkbtn" id="aboutVersion" title="Release page of this version">Version ${esc(S.version)}</button><span aria-hidden="true">·</span>
      <button class="linkbtn" id="aboutRepo">Source on GitHub</button><span aria-hidden="true">·</span>
      <button class="linkbtn" id="aboutIssues">Report a problem</button>
    </div>`);
  $("#themeSel", dlg).onchange = (e) => setTheme(e.target.value);
  $("#updChk", dlg).onchange = async (e) => {
    try {
      setState(await api("setSettings", { checkUpdates: e.target.checked }));
      if (e.target.checked) checkForUpdate();
      toast("Setting saved");
    } catch (err) { fail(err); }
  };
  $("#updNow", dlg).onclick = () => checkForUpdate(true);
  const openPage = (command, args) => T.core.invoke(command, args).catch((e) =>
    toast(typeof e === "string" ? e : "Couldn't open the browser.", true));
  $("#aboutVersion", dlg).onclick = () => openPage("open_release_page", { tag: `v${S.version}` });
  $("#aboutRepo", dlg).onclick = () => openPage("open_project_page", { page: "repo" });
  $("#aboutIssues", dlg).onclick = () => openPage("open_project_page", { page: "issues" });
  const saveFiles = async (change) => {
    try {
      setState(await scanning(api("setSettings", change)));
      $("#skipIn", dlg).value = S.settings.skipDirs.join(", ");
      $("#skipReset", dlg).disabled = sameList(S.settings.skipDirs, S.settings.defaultSkipDirs);
      toast("Setting saved");
    } catch (err) { fail(err); }
  };
  $("#depthSel", dlg).onchange = (e) => saveFiles({ scanDepth: +e.target.value });
  $("#skipIn", dlg).onchange = (e) => saveFiles({ skipDirs: e.target.value.split(",").map((d) => d.trim()).filter(Boolean) });
  $("#skipIn", dlg).onkeydown = (e) => { if (e.key === "Enter") e.target.blur(); };
  $("#skipReset", dlg).onclick = () => saveFiles({ skipDirs: S.settings.defaultSkipDirs });
  $("#clipSel", dlg).onchange = async (e) => {
    try { setState(await api("setSettings", { clipboardClearSeconds: +e.target.value })); toast("Setting saved"); } catch (err) { fail(err); }
  };
}

// ---- dialogs -------------------------------------------------------------
// A view that must not close by accident sets dlg.onleave after show(); the
// close button, the backdrop and Escape then call it instead of closing.
function show(dlg, title, html) {
  dlg.onleave = null;
  dlg.className = ""; // a size set for one view (see changesDialog) doesn't carry over

  dlg.innerHTML = `<div class="dhead"><span>${title}</span><span class="spacer"></span>
    <button class="btn icon" data-close aria-label="Close">${ic("x")}</button></div><div class="dbody">${html}</div>`;
  dlg.querySelector("[data-close]").onclick = () => (dlg.onleave ? dlg.onleave() : dlg.close());
  if (!dlg.open) dlg.showModal();
}
document.querySelectorAll("dialog").forEach((d) => {
  d.addEventListener("click", (e) => { if (e.target === d) d.onleave ? d.onleave() : d.close(); });
  d.addEventListener("cancel", (e) => { if (d.onleave) { e.preventDefault(); d.onleave(); } });
});

// What changed in the open file since the last commit: by variable for env
// files, by line for everything else. Values stay hidden until asked for.
async function changesDialog() {
  if (isDirty()) return toast("Save or discard your changes first.", true);
  let d;
  try { d = await api("changes", { path: cur.path }); } catch (e) { return fail(e); }
  const dlg = $("#dlg"), m = meta();
  const label = { added: "Added", changed: "Changed", removed: "Removed" };
  const plural = (n, word) => `${n} ${word}${n === 1 ? "" : "s"}`;
  // Text changes get a wide dialog whose list can be dragged taller, and a
  // button that grows the dialog to nearly the whole window.
  let reveal = false, large = false;
  const render = () => {
    const hidden = "••••••••";
    let rows, after = "";
    if (d.kind === "text") {
      rows = d.lines.map((l) => l.kind === "skip"
        ? `<div class="dl skip">${plural(l.count, "unchanged line")}</div>`
        : `<div class="dl ${l.kind}">${reveal ? esc(l.text) : l.text.trim() ? hidden : ""}</div>`).join("");
      if (rows) { rows = `<div class="diff">${rows}</div>`; after = `<p class="muted small">${plural(d.changed, "line")} added or removed</p>`; }
    } else {
      const val = (v) => `<span class="mono">${reveal ? esc(v) : hidden}</span>`;
      rows = d.changes.map((c) => `<div class="chg"><span class="pill chg-${c.kind}">${label[c.kind]}</span>
        <span class="mono ck">${esc(c.k)}</span>
        <span class="cv">${c.kind === "changed" ? `${val(c.old)} → ${val(c.new)}` : val(c.kind === "added" ? c.new : c.old)}</span></div>`).join("");
      if (rows && d.unchanged) after = `<p class="muted small">${plural(d.unchanged, "unchanged variable")}</p>`;
    }
    const name = `<span class="mono">${esc(m ? m.rel || m.name : "")}</span>`;
    show(dlg, "Changes since the last commit", `
      <p class="muted small">${d.committed ? `Compared with the committed version of ${name}.` : `${name} has never been committed, so everything in it is new.`}</p>
      ${rows || `<p>The content is the same as in the last commit. Only the encryption changed, for example the recipients.</p>`}
      ${after}
      ${rows ? `<div class="formrow" style="justify-content:flex-end">
        ${d.kind === "text" ? `<button class="btn" id="chgSize" title="${large ? "Back to the normal size" : "Use nearly the whole window"}">${ic(large ? "shrink" : "expand")}${large ? "Smaller" : "Larger"}</button>` : ""}
        <button class="btn" id="chgReveal">${ic(reveal ? "eyeoff" : "eye")}${reveal ? "Hide values" : "Show values"}</button></div>` : ""}`);
    if (d.kind === "text") dlg.classList.add("wide");
    if (large) dlg.classList.add("large");
    if ($("#chgReveal", dlg)) $("#chgReveal", dlg).onclick = () => { reveal = !reveal; render(); };
    if ($("#chgSize", dlg)) $("#chgSize", dlg).onclick = () => { large = !large; render(); };
  };
  render();
}

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
// Reads the folder again: new, removed and changed files appear, Git status
// is current. Runs on the refresh button and whenever the window gets focus
// back, for example after a git pull in a terminal. The open file reloads
// itself if it changed on disk and has no unsaved edits; with unsaved edits
// it keeps them, says so, and saving asks before overwriting.
let refreshing = null;
async function refresh(manual = false) {
  if (refreshing) return refreshing;
  // Don't pull the rug from under an open dialog or a save in progress.
  if (!manual && (document.querySelector("dialog[open]") || saving)) return;
  const btn = $("#refreshBtn");
  btn.classList.add("spin");
  refreshing = (async () => {
    try {
      setState(await scanning(api("state")));
      const m = meta();
      if (cur && !cur.blocked && m && m.rev && m.rev !== cur.rev) {
        if (isDirty()) {
          cur.stale = true;
          renderEditor();
        } else {
          const f = m;
          cur = null;
          await openFile(f);
          toast("Reloaded: the file changed on disk");
        }
      } else if (cur && !m && !cur.blocked && !cur.gone && !cur.isNew) {
        cur.gone = true;
        renderEditor();
      }
      if (manual) toast("Up to date");
    } catch (e) { fail(e); }
    finally { btn.classList.remove("spin"); refreshing = null; }
  })();
  return refreshing;
}
let lastFocusRefresh = 0;
function refreshOnFocus() {
  if (Date.now() - lastFocusRefresh < 2000) return;
  lastFocusRefresh = Date.now();
  refresh();
}

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
  if (cur && !cur.blocked && !cur.isNew && cur.recipients) {
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
      <input type="text" id="bk" class="mono" placeholder="age1…" spellcheck="false" aria-label="Public key"><button class="btn" id="badd">Save</button></div>
    <div class="formrow"><button class="btn" id="machineNew">${ic("plus")}Create new key pair</button></div>`);
  $("#machineNew", dlg).onclick = machineKeyDialog;
  const setRec = async (list, msg) => {
    if (isDirty()) return toast("Save or discard your changes first.", true);
    try {
      const d = await api("setRecipients", { path: cur.path, recipients: list });
      cur = { ...loaded(cur.path, d.file), revealed: cur.revealed };
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

// A new key pair for somewhere else that runs sops: a server, a pipeline, a
// colleague's container. The name is the user's; the app makes no assumption
// about where the key is used. The backend keeps the private
// key in memory; it goes from there straight to the clipboard and never
// reaches this page. Once copied (or discarded) it's forgotten, and the public
// key becomes a saved recipient.
let machinePending = null; // { copied } while a created key waits
function machineKeyDialog() {
  const dlg = $("#dlg");
  show(dlg, "New key pair", `<p class="muted small">Creates a key pair for another place that decrypts with sops. The public key is
    saved as a recipient. You copy the private key once to where it's needed; this app doesn't keep it.</p>
    <div class="formrow"><input type="text" id="mn" placeholder="Name, like production server" style="flex:1" aria-label="Name">
      <button class="btn primary" id="mc">${ic("key")}Create key</button></div>
    <div class="formrow" style="justify-content:flex-end"><button class="btn" id="mback">Back</button></div>`);
  $("#mn", dlg).focus();
  $("#mback", dlg).onclick = recipientsDialog;
  const create = async () => {
    try { machineKeyCreated(await api("machineKeyCreate", { name: $("#mn", dlg).value })); } catch (e) { fail(e); }
  };
  $("#mc", dlg).onclick = create;
  $("#mn", dlg).onkeydown = (e) => { if (e.key === "Enter") create(); };
}
function machineKeyCreated(made) {
  const dlg = $("#dlg");
  machinePending = { copied: false };
  const finish = async () => {
    try {
      const state = await api("machineKeyFinish");
      machinePending = null; await refreshAfter(state); recipientsDialog(); toast("Recipient saved");
    } catch (e) { fail(e); }
  };
  const discard = async () => {
    try { await api("machineKeyDiscard"); } catch { /* it's gone either way */ }
    machinePending = null; recipientsDialog();
  };
  // The close button, the backdrop and Escape: a copied key is saved, since
  // its private key may already be in use; an uncopied one is discarded on request.
  const leave = async () => {
    if (machinePending.copied) return finish();
    if (await ask("You haven't copied the private key. Without it, nothing encrypted for this key can be decrypted.\n\nDiscard the key?", "Discard")) discard();
  };
  const render = () => {
    const copied = machinePending.copied;
    show(dlg, "New key pair", `<div class="formrow" style="margin:6px 0 12px"><span class="muted">Name</span><strong id="mname">${esc(made.name)}</strong></div>
      <div class="note ${copied ? "ok" : "warn"}" id="mstate">${ic(copied ? "lock" : "alert")}<span>${copied
      ? "Private key copied. Store it now: it's gone once you close this dialog."
      : "Copy the private key now. It's shown only here, and the app forgets it when you close this dialog."}</span></div>
      <h3>Private key</h3>
      <div class="formrow" style="align-items:center;margin-top:0"><span class="mono secretmask" style="flex:1">AGE-SECRET-KEY-1••••••••••••••••</span>
        <button class="btn ${copied ? "" : "primary"}" id="mcopy">${ic("copy")}Copy private key</button></div>
      <p class="muted small">Where sops runs, provide it as the <span class="mono">SOPS_AGE_KEY</span> environment variable or in a
        <span class="mono">keys.txt</span> file.</p>
      <h3>Public key</h3>
      <div class="item" style="border:0;padding-top:0"><div class="sub"><span class="mono" id="mpub">${esc(made.pub)}</span>
        <button class="btn icon" id="mpubcopy" title="Copy public key" aria-label="Copy public key">${ic("copy")}</button></div></div>
      <div class="formrow" style="justify-content:flex-end"><button class="btn" id="mdiscard">Discard</button>
        <button class="btn primary" id="mdone" ${copied ? "" : `disabled title="Copy the private key first"`}>Save recipient</button></div>`);
    dlg.onleave = leave;
    $("#mcopy", dlg).onclick = async () => {
      try {
        const seconds = await T.core.invoke("copy", { secret: true, machineKey: true });
        machinePending.copied = true; render();
        toast(seconds ? `Private key copied. The clipboard is cleared in ${duration(seconds)}.` : "Private key copied");
      } catch (e) { toast(typeof e === "string" ? e : "Couldn't copy to the clipboard.", true); }
    };
    $("#mpubcopy", dlg).onclick = () => copy(made.pub, "Public key");
    $("#mdone", dlg).onclick = finish;
    $("#mdiscard", dlg).onclick = discard;
  };
  render();
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

$("#updateBtn").innerHTML = ic("upgrade");
$("#updateBtn").onclick = updateDialog;
$("#settingsBtn").innerHTML = ic("gear");
$("#settingsBtn").onclick = settingsDialog;
$("#keysBtn").onclick = keysDialog;
$("#recBtn").onclick = recipientsDialog;
async function openFolder(path) {
  try { cur = null; renderEditor(); setState(await scanning(api("setFolder", { path }))); } catch (e) { fail(e); }
}
async function chooseFolder() {
  const p = await pick("folder", "Choose a project folder", S.folder);
  if (p) openFolder(p);
}
$("#folderBtn").onclick = async () => {
  if (isDirty() && !(await ask("Discard unsaved changes?", "Discard"))) return;
  const recent = (S.recent || []).filter((f) => f !== S.folder);
  if (!recent.length) return chooseFolder();
  const dlg = $("#dlg");
  show(dlg, "Open folder", `<h3>Recent</h3>
    <div class="recent">${recent.map((f, i) => `<button class="file" data-r="${i}" title="${esc(f)}">${ic("folder")}<span class="mono"><bdi>${esc(f)}</bdi></span></button>`).join("")}</div>
    <div class="formrow" style="justify-content:flex-end"><button class="btn" id="otherFolder">${ic("folder")}Choose another folder</button></div>`);
  dlg.querySelectorAll("[data-r]").forEach((b) => (b.onclick = () => { dlg.close(); openFolder(recent[b.dataset.r]); }));
  $("#otherFolder", dlg).onclick = () => { dlg.close(); chooseFolder(); };
};

// Ask before closing the window with unsaved edits.
try {
  T.window.getCurrentWindow().onCloseRequested(async (event) => {
    if (isDirty() && !(await ask("Close and discard unsaved changes?", "Discard"))) return event.preventDefault();
    if (machinePending && !machinePending.copied &&
      !(await ask("You haven't copied the private key of the new key pair. Close and discard it?", "Discard"))) event.preventDefault();
  });
} catch { /* closing simply won't ask */ }

$("#refreshBtn").innerHTML = ic("refresh");
$("#refreshBtn").onclick = () => refresh(true);
// Coming back to the window is when files most likely changed outside.
window.addEventListener("focus", refreshOnFocus);
try { T.window.getCurrentWindow().onFocusChanged(({ payload }) => { if (payload) refreshOnFocus(); }); } catch { /* the window event above covers it */ }

scanning(api("state")).then((s) => {
  setState(s); renderEditor();
  checkForUpdate();
  setInterval(() => checkForUpdate(), DAY);
}).catch(fail);

// Scripted self-test: loaded only when the app was started with OPEN_SOPS_UI_SELFTEST set.
T.core.invoke("ui_test", { action: "config", body: {} }).then((on) => {
  if (on) document.body.appendChild(Object.assign(document.createElement("script"), { src: "selftest.js" }));
}).catch(() => {});
