// Scripted UI self-test. app.js loads this file only when the app was started
// with OPEN_SOPS_UI_SELFTEST set; it never runs for a normal user.
//
// It clicks through the main flows in the real window, takes a screenshot at
// each stage, and reports to the backend, which writes result.json and exits.
// ui-tests/selftest.mjs prepares the test folder and checks the files on disk.
(async () => {
  const call = (action, body = {}) => window.__TAURI__.core.invoke("ui_test", { action, body });
  const q = (selector) => document.querySelector(selector);
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const log = [];

  async function until(what, probe, timeout = 20000) {
    const end = Date.now() + timeout;
    while (Date.now() < end) {
      const value = probe();
      if (value) return value;
      await sleep(100);
    }
    throw new Error(`timed out waiting for ${what}`);
  }
  const text = (selector) => q(selector)?.innerText ?? "";
  const shows = (selector, part) => until(`"${part}" in ${selector}`, () => text(selector).includes(part));

  // The sidebar entry for a file, by its name.
  const fileBtn = (name) => [...document.querySelectorAll("#files .file[data-i]")].find((b) => b.querySelector(".mono").innerText === name);
  const kinds = () => [...document.querySelectorAll("#dlg .chg")].map((r) => `${r.querySelector(".pill").innerText.trim()} ${r.querySelector(".ck").innerText.trim()}`);

  async function click(selector) {
    (await until(`element ${selector}`, () => q(selector))).click();
  }
  async function type(selector, value) {
    const el = await until(`element ${selector}`, () => q(selector));
    el.focus();
    el.value = value;
    el.dispatchEvent(new Event("input", { bubbles: true }));
  }
  async function shot(name) {
    // A lingering toast from an earlier step would mislead in a still picture.
    q("#toast").className = "";
    // Let the page paint what the last step changed before capturing it.
    await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
    await sleep(350);
    const saved = await call("shot", { name });
    log.push(`${saved ? "shot" : "NO SHOT"} ${name}`);
  }
  function check(condition, message) {
    if (!condition) throw new Error(`check failed: ${message}`);
    log.push(`ok ${message}`);
  }

  try {
    setTheme("light");
    await until("the file list", () => q("#files .file[data-i]"));
    check(text("#files").includes(".env.production") && text("#files").includes(".env.staging"), "both env files are listed");
    const dir = q("#files .dir");
    check(dir && /services[\\/]worker[\\/]$/.test(dir.innerText.trim()), "a file in a subfolder is grouped under its folder");
    const sub = q("#files .dir.sub");
    check(sub && /^config[\\/]$/.test(sub.innerText.trim()), "a nested folder shows only its own part, under its parent");
    check([...document.querySelectorAll("#files .file[data-i] .mono")].every((e) => !/[\\/]/.test(e.innerText)), "file entries show the name without the path");
    const repo = q("#files .repo");
    check(repo && repo.innerText.includes("shop-api") && repo.innerText.includes("main"), "the repository is shown with its branch");
    check(fileBtn(".env.example").classList.contains("s-plain") && !fileBtn(".env.example").querySelector(".gs"), "a committed example file raises no warning");
    check(fileBtn(".env.staging").classList.contains("s-plain"), "an ignored plaintext file raises no warning");
    check(fileBtn(".env.production").classList.contains("s-plain-risk"), "plaintext that Git doesn't ignore is flagged");
    check(text(".sopsinfo").startsWith("sops "), "the app found sops");
    check(q("#keysDot").className === "dot", "a missing key is flagged");
    await shot("01-start-without-key");

    await click("#keysBtn");
    await click("#dlg [data-f=new]");
    await until("the create-key form", () => q("#dlg #kp"));
    await shot("02-create-key");
    await click("#dlg #kp");
    await until("the key in the dialog", () => q("#dlg [data-copy]"));
    check(q("#keysDot").className === "", "the new key is loaded");
    await shot("03-keys");
    await click("#dlg [data-close]");

    fileBtn(".env.production").click();
    await until("the variables", () => q("#editor .row"));
    check(text(".note").includes("Git isn't ignoring it") && q("#ignoreBtn"), "the plaintext warning offers to ignore the file");
    check(document.querySelectorAll("#editor .row").length === 4, "all four variables are shown");
    check(text(".filehead").includes("Not encrypted yet"), "the file is shown as plaintext");
    await shot("04-plaintext-file");

    await click("#save");
    await until("the recipient chooser", () => q("#dlg #go"));
    await shot("05-encrypt-for");
    await click("#dlg #go");
    await shows(".filehead", "1 recipient");
    check(fileBtn(".env.production").classList.contains("s-ok"), "the file is marked as encrypted and readable");
    check(fileBtn(".env.production").querySelector(".gs")?.innerText === "N", "the file is marked as not committed yet");
    await shot("06-encrypted");

    await click("#changesBtn");
    await until("the changes", () => q("#dlg[open] .chg")); // [open]: a closed dialog keeps its old content
    check(kinds().length === 4 && kinds().every((k) => k.startsWith("Added")), "a never-committed file lists every variable as added");
    check(!text("#dlg").includes("sk_test_1"), "values in the comparison are hidden at first");
    await click("#dlg [data-close]");
    check(await call("commit", { path: cur.path }), "the test committed the encrypted file");

    await click("#editor .row:nth-of-type(2) [data-a=eye]");
    await type("#editor .row:nth-of-type(2) .val", "sk_test_changed");
    await click("#addVar");
    await type("#editor .row:nth-of-type(5) .k", "SENTRY_DSN");
    await type("#editor .row:nth-of-type(5) .val", "https://abc123@sentry.example/42");
    await shows("#dirty", "Unsaved");
    check(q("#editor .row:nth-of-type(2)").classList.contains("changed"), "the edited row is marked");
    check(q("#editor .row:nth-of-type(5)").classList.contains("added"), "the new row is marked");
    await shot("07-unsaved-changes");

    // Someone else changes the file while it is open here: saving must ask first.
    const elsewhere = JSON.parse(cur.orig).map((i) => (i.k === "SMTP_PASSWORD" ? { ...i, v: "changed-elsewhere" } : i));
    await api("save", { path: cur.path, items: elsewhere });
    await click("#save");
    await shows("#dlg2", "changed outside this app");
    check(text("#dirty").includes("Unsaved"), "a file changed on disk is not overwritten without asking");
    await shot("08-changed-on-disk");
    await click("#cfOverwrite");
    await shows("#dirty", "Saved");
    check(document.querySelectorAll("#editor .row").length === 5, "five variables after saving");
    check(cur.items.find((i) => i.k === "SMTP_PASSWORD").v === "mail-pass-1234", "overwriting keeps the version shown in the app");

    await until("the modified mark", () => fileBtn(".env.production").querySelector(".gs")?.innerText === "M");
    await click("#changesBtn");
    await until("the changes", () => q("#dlg[open] .chg")); // [open]: a closed dialog keeps its old content
    check(kinds().join(", ") === "Changed STRIPE_SECRET, Added SENTRY_DSN", "the comparison lists exactly what changed since the commit");
    check(text("#dlg").includes("3 unchanged"), "unchanged variables are counted, not listed");
    await shot("09-changes-since-commit");
    await click("#chgReveal");
    await shows("#dlg", "sk_test_changed");
    check(text("#dlg").includes("sk_test_1"), "revealing shows the old and the new value");
    await click("#dlg [data-close]");

    await click("#recBtn");
    await shows("#dlg", "You (default)");
    await shot("10-recipients");
    await click("#dlg [data-close]");

    // The other answer to a changed file: reload it and drop the local edit.
    const other = JSON.parse(cur.orig).map((i) => (i.k === "JWT_SIGNING_KEY" ? { ...i, v: "rotated-elsewhere" } : i));
    await api("save", { path: cur.path, items: other });
    await type("#editor .row:nth-of-type(1) .val", "my-edit");
    await click("#save");
    await click("#cfReload");
    await until("the reloaded file", () => cur && cur.items.find((i) => i.k === "JWT_SIGNING_KEY")?.v === "rotated-elsewhere");
    check(!text("#dirty").includes("Unsaved") && !cur.items.some((i) => i.v === "my-edit"), "reloading shows the version on disk and drops the edit");
    await type("#editor .row:nth-of-type(3) .val", "6f1e0c9a4b7d2e8f5a3c1b9d");
    await click("#save");
    await shows("#dirty", "Saved");
    check(!q("#dlg2").open, "after reloading, saving needs no confirmation");

    // Files change outside the app, as after a git pull. The refresh button and
    // getting the window's focus back both show them.
    const dirOf = (p) => p.slice(0, Math.max(p.lastIndexOf("/"), p.lastIndexOf("\\")) + 1);
    const project = dirOf(cur.path);
    await call("writeFile", { path: project + ".env.preview", text: "PREVIEW_URL=https://preview.example\n" });
    check(!fileBtn(".env.preview"), "a file added outside the app isn't listed yet");
    await click("#refreshBtn");
    await until("the new file", () => fileBtn(".env.preview"));
    check(true, "the refresh button lists a file added outside the app");
    await call("writeFile", { path: project + ".env.qa", text: "QA=1\n" });
    await sleep(2100); // focus refreshes are at most every two seconds
    window.dispatchEvent(new Event("focus"));
    await until("the file found on focus", () => fileBtn(".env.qa"));
    check(true, "getting the window's focus back lists new files");

    // The open file changes on disk without local edits: it reloads by itself.
    const pulled = JSON.parse(cur.orig).map((i) => (i.k === "SMTP_PASSWORD" ? { ...i, v: "pulled-value" } : i));
    await api("save", { path: cur.path, items: pulled });
    await sleep(2100);
    window.dispatchEvent(new Event("focus"));
    await until("the reloaded value", () => cur && cur.items.find((i) => i.k === "SMTP_PASSWORD")?.v === "pulled-value");
    check(!q("#dlg2").open && !text("#dirty").includes("Unsaved"), "an open file without edits reloads when it changed on disk");

    // With local edits, they are kept and a note says the file changed.
    await type("#editor .row:nth-of-type(1) .val", "local-edit");
    const pulledAgain = JSON.parse(cur.orig).map((i) => (i.k === "SMTP_PASSWORD" ? { ...i, v: "pulled-again" } : i));
    await api("save", { path: cur.path, items: pulledAgain });
    await click("#refreshBtn");
    await shows(".note", "changed on disk while you were editing");
    check(text("#dirty").includes("Unsaved") && cur.items.some((i) => i.v === "local-edit"), "local edits survive a refresh");
    await shot("11-changed-while-editing");
    await click("#revert");
    await until("the version on disk", () => cur && !cur.stale && cur.items.find((i) => i.k === "SMTP_PASSWORD")?.v === "pulled-again");
    check(!cur.items.some((i) => i.v === "local-edit"), "discarding after an outside change takes the version on disk");
    // Put the value the file checks expect back.
    await type("#editor .row:nth-of-type(4) .val", "mail-pass-1234");
    await click("#save");
    await shows("#dirty", "Saved");
    q("#toast").className = "";

    // Copying a value: it reaches the clipboard and is removed again.
    setState(await api("setSettings", { clipboardClearSeconds: 2 }));
    await click("#editor .row:nth-of-type(2) [data-a=copy]");
    await shows("#toast", "cleared in 2 s");
    check((await call("clipboard")) === "sk_test_changed", "the copied value is on the clipboard");
    await sleep(4500);
    check((await call("clipboard")) !== "sk_test_changed", "the clipboard no longer holds the value after the set time");
    q("#toast").className = "";

    await type("#editor .row:nth-of-type(1) .val", "unsaved");
    await click("#folderBtn");
    await shows("#dlg2", "Discard unsaved changes?");
    await shot("12-confirm");
    await click("#dlg2 #no");
    await click("#revert");
    check(!text("#dirty").includes("Unsaved"), "discarding restores the saved values");

    await click("#folderBtn");
    await shows("#dlg", "blog");
    check(document.querySelectorAll("#dlg [data-r]").length === 1, "the other recently opened folder is offered");
    await shot("13-recent-folders");
    await click("#dlg [data-close]");

    await click("#settingsBtn");
    await until("the settings", () => q("#dlg[open] #clipSel"));
    const sel = q("#clipSel");
    sel.value = "60";
    sel.dispatchEvent(new Event("change", { bubbles: true }));
    await until("the saved setting", () => S.settings.clipboardClearSeconds === 60);
    check(true, "the clipboard time can be changed in the settings");
    q("#toast").className = "";
    check(!q("#themeBtn"), "the theme switch lives in the settings, not the header");
    await shot("14-settings");
    const themeSel = q("#themeSel");
    themeSel.value = "dark";
    themeSel.dispatchEvent(new Event("change", { bubbles: true }));
    check(document.documentElement.dataset.theme === "dark", "choosing dark in the settings applies dark mode");
    await click("#dlg [data-close]");
    await click("#settingsBtn");
    await until("the settings", () => q("#dlg[open] #themeSel"));
    check(q("#themeSel").value === "dark", "the settings show the chosen theme when opened again");
    await click("#dlg [data-close]");
    await shot("15-dark");
    await click("#keysBtn");
    await until("the key in the dialog", () => q("#dlg [data-copy]"));
    await shot("16-dark-keys");
    await click("#dlg [data-close]");
    await click("#changesBtn");
    await until("the changes", () => q("#dlg[open] .chg"));
    await shot("17-dark-changes");
    await click("#dlg [data-close]");

    await call("done", { ok: true, log });
  } catch (e) {
    log.push(`FAILED ${e.message}`);
    try {
      await shot("99-failure");
    } catch { /* report without it */ }
    await call("done", { ok: false, error: e.message, log });
  }
})();
