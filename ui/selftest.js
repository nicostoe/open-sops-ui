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
      const value = await probe(); // a probe may be async
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
    for (const name of [".env.production", ".env.staging", "recovery-codes.txt", "app.yaml", "feature-flags.json", "billing.ini"]) {
      check(fileBtn(name), `${name} is listed`);
    }
    check(!fileBtn(".env.example"), "a plaintext file isn't listed");
    check(!q("aside").classList.contains("scanning"), "the scan bar is gone once the files are listed");
    const dirs = [...document.querySelectorAll("#files .dir")].map((d) => d.innerText.trim());
    check(dirs.some((d) => /^services[\\/]worker[\\/]$/.test(d)), "a file in a subfolder is grouped under its folder");
    const sub = q("#files .dir.sub");
    check(sub && /^config[\\/]$/.test(sub.innerText.trim()), "a nested folder shows only its own part, under its parent");
    check([...document.querySelectorAll("#files .file[data-i] .mono")].every((e) => !/[\\/]/.test(e.innerText)), "file entries show the name without the path");
    const repo = q("#files .repo");
    check(repo && repo.innerText.includes("shop-api") && repo.innerText.includes("main"), "the repository is shown with its branch");
    check(fileBtn(".env.production").classList.contains("s-locked"), "without a key, the files are locked");
    check(fileBtn(".env.production").querySelector(".gs")?.innerText === "N", "a file that isn't committed yet is marked");
    check(!fileBtn("app.yaml").querySelector(".gs"), "a committed file has no mark");
    check(text(".sopsinfo").startsWith("sops "), "the app found sops");
    check(q("#keysDot").className === "dot", "a missing key is flagged");
    await shot("01-start-without-key");

    // The scan bar: shown while a refresh runs (after a short delay, so a quick one doesn't flicker).
    const running = refresh(true);
    check(q("aside").classList.contains("scanning"), "a refresh shows the scan bar");
    await running;
    check(!q("aside").classList.contains("scanning"), "the scan bar goes away when the scan is done");
    q("aside").classList.add("scanning"); // held on for the picture, as on a slow scan
    await shot("01a-scanning");
    q("aside").classList.remove("scanning");

    // The file list's width follows its drag handle, stays within limits and is remembered.
    const resizer = q("#resizer"), sideW = () => Math.round(q("aside").getBoundingClientRect().width);
    const drag = (dx) => {
      const x = resizer.getBoundingClientRect().left + 3;
      resizer.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true, button: 0, clientX: x, pointerId: 7 }));
      window.dispatchEvent(new PointerEvent("pointermove", { clientX: x + dx, pointerId: 7 }));
      window.dispatchEvent(new PointerEvent("pointerup", { clientX: x + dx, pointerId: 7 }));
    };
    resizer.dispatchEvent(new MouseEvent("dblclick", { bubbles: true })); // a width left by an earlier run
    const before = sideW();
    check(before === 250, "the file list starts at its default width");
    drag(120);
    check(sideW() === before + 120, "dragging the handle widens the file list");
    check(localStorage.getItem("sideWidth") === String(before + 120), "the chosen width is remembered");
    check(Math.round(resizer.getBoundingClientRect().left + 3) === Math.round(q("aside").getBoundingClientRect().right), "the handle follows the list's edge");
    drag(-2000);
    check(sideW() === 160, "the file list can't be dragged narrower than its minimum");
    drag(5000);
    check(sideW() <= 600 && q("#editor").getBoundingClientRect().width >= 360, "a wide file list leaves the editor room");
    resizer.dispatchEvent(new KeyboardEvent("keydown", { key: "Home", bubbles: true }));
    resizer.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true }));
    check(sideW() === 176, "the arrow keys move the handle");
    resizer.dispatchEvent(new MouseEvent("dblclick", { bubbles: true }));
    check(sideW() === 250 && localStorage.getItem("sideWidth") === null, "double-clicking the handle restores the default width");

    await click("#keysBtn");
    await click("#dlg [data-f=new]");
    await until("the create-key form", () => q("#dlg #kp"));
    await shot("02-create-key");
    await click("#dlg #kp");
    await until("the key in the dialog", () => q("#dlg [data-copy]"));
    check(q("#keysDot").className === "", "the new key is loaded");
    await shot("03-keys");
    await click("#dlg [data-close]");

    // The project's files are encrypted for a teammate's key: opening one says so.
    fileBtn(".env.production").click();
    await until("the missing key", () => q("#editor #goKeys"));
    check(text("#editor").includes("No matching key"), "a file without a matching key explains why it doesn't open");
    await shot("04-no-matching-key");
    const fx = await call("fixture");
    await click("#goKeys");
    await click("#dlg [data-f=paste]");
    await type("#dlg #kc", fx.teamKey);
    await type("#dlg #kl", "team");
    await click("#dlg #ks");
    await until("the variables", () => q("#editor .row"));
    check(document.querySelectorAll("#editor .row").length === 4, "with the teammate's key, all four variables are shown");
    check(fileBtn(".env.production").classList.contains("s-ok"), "the file is marked as readable");
    check(text(".filehead").includes("1 recipient"), "the file has one recipient");
    await until("the keys dialog", () => q("#dlg[open] [data-persist]"));
    await click("#dlg [data-close]");
    await shot("05-env-file");

    // Show all / hide all values at once; revealing every row one by one counts too.
    const valueTypes = () => [...document.querySelectorAll("#editor .row .val")].map((v) => v.type);
    check(valueTypes().every((t) => t === "password") && text("#revealAll").includes("Show all"), "values start hidden, with a Show all button");
    await click("#revealAll");
    await until("every value shown", () => valueTypes().every((t) => t === "text"));
    check(text("#revealAll").includes("Hide all"), "Show all reveals every value and turns into Hide all");
    await shot("05a-env-all-shown");
    await click("#revealAll");
    await until("every value hidden", () => valueTypes().every((t) => t === "password"));
    for (let i = 1; i <= 4; i++) await click(`#editor .row:nth-of-type(${i}) [data-a=eye]`);
    check(valueTypes().every((t) => t === "text") && text("#revealAll").includes("Hide all"), "revealing each value by hand also offers Hide all");
    await click("#revealAll");
    await until("every value hidden again", () => valueTypes().every((t) => t === "password"));
    check(text("#revealAll").includes("Show all"), "Hide all hides every value");

    // Add my own key as a recipient, so the file opens without the teammate's key.
    await click("#recBtn");
    await until("the recipient choice", () => q("#dlg[open] #addSel"));
    const mine = [...q("#addSel").options].find((o) => o.textContent.startsWith("You (default)"));
    check(mine, "my own key can be added as a recipient");
    q("#addSel").value = mine.value;
    await click("#dlg #addRec");
    await shows(".filehead", "2 recipients");
    await until("the updated chips", () => document.querySelectorAll("#dlg[open] .chip").length === 2);
    await shot("06-recipient-added");
    await click("#dlg [data-close]");

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
    check(!q("#dlg").classList.contains("wide") && !q("#dlg #chgSize"), "the variable comparison keeps the normal dialog size");
    await shot("09-changes-since-commit");
    await click("#chgReveal");
    await shows("#dlg", "sk_test_changed");
    check(text("#dlg").includes("sk_test_1"), "revealing shows the old and the new value");
    await click("#dlg [data-close]");

    await click("#recBtn");
    await shows("#dlg", "You (default)");
    await shot("10-recipients");

    // A new key pair for somewhere else: the private key goes to the clipboard without
    // ever being in the page, and the public key becomes a saved recipient.
    const secretInPage = () => /AGE-SECRET-KEY-1[0-9A-Z]{8}/.test(document.documentElement.outerHTML);
    await click("#machineNew");
    await until("the server key form", () => q("#dlg[open] #mn"));
    await click("#dlg #mc");
    await until("the missing name message", () => text("#toast").includes("Enter a name"));
    check(!q("#dlg #mcopy"), "a new key pair needs a name");
    await type("#dlg #mn", "Production server");
    await shot("10a-new-key-pair-name");
    await click("#dlg #mc");
    await until("the created key", () => q("#dlg[open] #mcopy"));
    const ciPub = text("#mpub");
    check(ciPub.startsWith("age1") && !secretInPage(), "only the public key of a new key pair is in the page");
    check(q("#dlg .dhead").innerText.trim() === "New key pair" && text("#mname") === "Production server", "the dialog has a generic title and shows the chosen name");
    check(q("#dlg #mdone").disabled, "the recipient can't be saved before the private key was copied");
    await shot("10b-new-key-pair");
    await click("#dlg [data-close]");
    await until("the question about the uncopied key", () => q("#dlg2[open] #no") && text("#dlg2").includes("haven't copied"));
    await click("#dlg2 #no");
    check(q("#dlg").open && q("#dlg #mcopy"), "closing before the private key was copied asks, and No keeps the key");
    await click("#dlg #mcopy");
    await until("the copied state", () => q("#dlg[open] #mstate.ok"));
    const ciSecret = await call("clipboard");
    check(ciSecret.startsWith("AGE-SECRET-KEY-1") && !secretInPage(), "the private key is on the clipboard but not in the page");
    const loaded = await api("addSessionKey", { content: ciSecret, label: "check" }); // test only: prove the pair belongs together
    check(loaded.state.keys.some((k) => k.pubs.includes(ciPub)), "the copied private key belongs to the shown public key");
    setState(await api("removeKey", { id: loaded.id }));
    check(!q("#dlg #mdone").disabled, "after copying, the recipient can be saved");
    await click("#dlg #mdone");
    await until("the recipients again", () => q("#dlg[open] #machineNew"));
    check(S.book.some((b) => b.name === "Production server" && b.key === ciPub) &&
      [...document.querySelectorAll("#dlg .item")].some((i) => i.innerText.includes("Production server") && i.innerText.includes(ciPub)),
      "Save recipient goes back to the recipients, with the new one listed");
    // Discard goes straight back and saves nothing, copied or not.
    await click("#machineNew");
    await type("#dlg #mn", "staging");
    await click("#dlg #mc");
    await until("the second key", () => q("#dlg[open] #mcopy"));
    await click("#dlg #mcopy");
    await until("the copied state", () => q("#dlg[open] #mstate.ok"));
    await click("#dlg #mdiscard");
    await until("the recipients again", () => q("#dlg[open] #machineNew"));
    check(!q("#dlg2").open && !S.book.some((b) => b.name === "staging"), "Discard goes back to the recipients without saving");
    // Closing a key that was never copied, and confirming, saves nothing.
    await click("#machineNew");
    await type("#dlg #mn", "staging");
    await click("#dlg #mc");
    await until("the second key", () => q("#dlg[open] #mcopy"));
    await click("#dlg [data-close]");
    await until("the question", () => q("#dlg2[open] #yes"));
    await click("#dlg2 #yes");
    await until("the recipients again", () => q("#dlg[open] #machineNew"));
    check(!S.book.some((b) => b.name === "staging"), "closing an uncopied key pair discards it");
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
    await call("writeFile", { path: project + ".env.preview", text: fx.preview });
    await call("writeFile", { path: project + ".env.local", text: "LOCAL=plaintext\n" });
    check(!fileBtn(".env.preview"), "a file added outside the app isn't listed yet");
    await click("#refreshBtn");
    await until("the new file", () => fileBtn(".env.preview"));
    check(true, "the refresh button lists a file added outside the app");
    check(!fileBtn(".env.local"), "a plaintext file added outside the app isn't listed");
    await call("writeFile", { path: project + ".env.qa", text: fx.qa });
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

    // A new encrypted file, created from the sidebar.
    await click("#newFile");
    await until("the new-file dialog", () => q("#dlg[open] #nfName"));
    await type("#nfName", "../x.env");
    await click("#nfGo");
    await shows("#nfErr", "without folders");
    check(q("#dlg").open, "a name with a folder in it is refused with a hint");
    await type("#nfName", ".env.ci");
    await shot("14-new-file");
    await click("#nfGo");
    await until("the empty editor", () => !q("#dlg").open && q("#editor .row"));
    check(text(".filehead").includes(".env.ci") && text(".filehead").includes("New file"), "the editor opens for the new file");
    await type("#editor .row .k", "CI_TOKEN");
    await type("#editor .row .val", "abc123");
    await click("#save");
    await until("the recipient chooser", () => q("#dlg[open] #go"));
    await shot("14a-encrypt-for");
    await click("#dlg #go");
    await shows(".filehead", "recipients");
    check(fileBtn(".env.ci")?.classList.contains("s-ok"), "the new file is listed, encrypted and readable");
    q("#toast").className = "";

    // Other formats open as text: the whole content in one field, masked until shown.
    fileBtn("recovery-codes.txt").click();
    await until("the text editor", () => cur && cur.path.endsWith("recovery-codes.txt") && q("#editor #textEdit"));
    check(q("#codeWrap").classList.contains("masked"), "the content is masked at first");
    check(q("#textEdit").value.includes("Recovery codes"), "the decrypted text is in the editor");
    check(text(".filehead .fmt") === "TEXT" || text(".filehead .fmt") === "Text", "the format is shown");
    check(!q("#editor .row") && !q("#addVar"), "a text file has no variable table");
    check(!q("#codeWrap").classList.contains("coloured"), "masked text isn't coloured");
    await shot("14b-text-file-masked");
    await click("#revealText");
    check(!q("#codeWrap").classList.contains("masked"), "Show reveals the content");
    check(!q("#codeWrap").classList.contains("coloured"), "a text file without a known syntax stays plain");
    const area = q("#textEdit");
    area.focus();
    area.value += "c64f-1e8b-77a2\n";
    area.dispatchEvent(new Event("input", { bubbles: true }));
    await shows("#dirty", "Unsaved");
    await shot("14c-text-file");
    await click("#save");
    await shows("#dirty", "Saved");
    check(q("#textEdit").value.endsWith("77a2\n") && !q("#codeWrap").classList.contains("masked"), "saving keeps the content and the view");
    await until("the modified mark", () => fileBtn("recovery-codes.txt").querySelector(".gs")?.innerText === "M");
    await click("#changesBtn");
    await until("the line changes", () => q("#dlg[open] .diff"));
    const diff = [...document.querySelectorAll("#dlg .dl")].map((l) => l.className.replace("dl ", ""));
    check(diff.filter((k) => k === "added").length === 1 && !diff.includes("removed"), "the comparison shows the one added line");
    check(!text("#dlg .diff").includes("c64f-1e8b"), "the lines are hidden at first");
    await click("#chgReveal");
    await shows("#dlg .diff", "c64f-1e8b-77a2");
    const box = (sel) => q(sel).getBoundingClientRect();
    check(q("#dlg").classList.contains("wide") && box("#dlg").width > 700, "the text comparison gets a wide dialog");
    check(getComputedStyle(q("#dlg .diff")).resize === "vertical", "the changed lines can be dragged taller");
    await shot("14d-text-changes");
    const normal = box("#dlg .diff").height;
    await click("#dlg #chgSize");
    await until("the large dialog", () => q("#dlg.large[open] #chgSize"));
    check(box("#dlg").height > innerHeight - 60 && box("#dlg .diff").height > normal, "Larger grows the dialog to nearly the window, the list with it");
    check(text("#dlg .diff").includes("c64f-1e8b-77a2"), "the shown values stay shown at the larger size");
    await shot("14d2-text-changes-large");
    await click("#dlg #chgSize");
    await until("the normal size again", () => !q("#dlg").classList.contains("large") && q("#dlg[open] #chgSize"));
    await click("#dlg [data-close]");
    // Discarding text edits brings back the saved content.
    q("#textEdit").value = "scratch";
    q("#textEdit").dispatchEvent(new Event("input", { bubbles: true }));
    await click("#revert");
    check(q("#textEdit").value.endsWith("77a2\n") && !text("#dirty").includes("Unsaved"), "Discard restores the saved text");

    for (const [name, part, label, picture] of [
      ["app.yaml", "session_secret", "YAML", "14e-yaml-file"],
      ["feature-flags.json", "ff_live_51c0", "JSON", "14f-json-file"],
      ["billing.ini", "sk_live_6a2e", "INI", "14g-ini-file"],
    ]) {
      fileBtn(name).click();
      await until(`the ${label} file`, () => cur && cur.path.endsWith(name) && q("#editor #textEdit"));
      check(q("#textEdit").value.includes(part) && text(".filehead .fmt").toUpperCase() === label, `${name} opens as ${label} text`);
      await click("#revealText");
      // Coloured syntax: the layer behind the textarea holds exactly its text.
      check(q("#codeWrap").classList.contains("coloured") && q("#codeWrap .hl .t-key") && q("#codeWrap .hl .t-str"), `${label} is coloured`);
      check(q("#codeWrap .hl code").textContent === q("#textEdit").value + "\n", `the coloured ${label} lines up with the text`);
      const box = (sel) => q(sel).getBoundingClientRect();
      check(box("#codeWrap .hl").top === box("#textEdit").top && box("#codeWrap .hl").left === box("#textEdit").left, `the ${label} colour layer covers the textarea`);
      await shot(picture);
    }
    check(q("#codeWrap .hl .t-section"), "INI sections are coloured");
    // Saving YAML encrypts it again, for the same recipient.
    fileBtn("app.yaml").click();
    await until("the YAML file", () => cur && cur.path.endsWith("app.yaml") && q("#editor #textEdit"));
    check(q("#codeWrap").classList.contains("masked"), "a file opened again is masked again");
    await click("#revealText");
    q("#textEdit").value = q("#textEdit").value.replace("smtp.example", "smtp2.example").trimEnd() + "\n# rotated\n";
    q("#textEdit").dispatchEvent(new Event("input", { bubbles: true }));
    check(q("#codeWrap .hl").textContent.includes("smtp2.example") && [...document.querySelectorAll("#codeWrap .t-comment")].some((c) => c.textContent === "# rotated"),
      "the colours follow typing");
    await click("#save");
    await shows("#dirty", "Saved");
    check(cur.text.includes("smtp2.example") && cur.recipients.length === 1, "a YAML file saves, for the same recipient");

    fileBtn(".env.production").click();
    await until("the production file again", () => cur && cur.path.endsWith(".env.production") && q("#editor .row"));
    q("#toast").className = "";

    await click("#settingsBtn");
    await until("the settings", () => q("#dlg[open] #clipSel"));
    check(!q("#dlg").classList.contains("wide") && q("#dlg").getBoundingClientRect().width <= 600, "a dialog opened later has its normal size again");
    const sel = q("#clipSel");
    sel.value = "60";
    sel.dispatchEvent(new Event("change", { bubbles: true }));
    await until("the saved setting", () => S.settings.clipboardClearSeconds === 60);
    check(true, "the clipboard time can be changed in the settings");
    q("#toast").className = "";
    // How deep the file list searches, and which folders it skips.
    const depth = q("#depthSel");
    check(depth.value === "3", "the search depth starts at three levels");
    depth.value = "1";
    depth.dispatchEvent(new Event("change", { bubbles: true }));
    await until("the shallow file list", () => S.settings.scanDepth === 1 && !fileBtn(".env.testing"));
    check(fileBtn("recovery-codes.txt") && !fileBtn(".env.testing"), "a lower depth leaves out deeper files");
    depth.value = "3";
    depth.dispatchEvent(new Event("change", { bubbles: true }));
    await until("the deep files again", () => fileBtn(".env.testing"));
    const skip = q("#skipIn");
    check(skip.value.includes("node_modules") && q("#skipReset").disabled, "the skipped folders start with the defaults");
    skip.value = skip.value + ", config";
    skip.dispatchEvent(new Event("change", { bubbles: true }));
    await until("the config folder skipped", () => !fileBtn("app.yaml"));
    check(!fileBtn(".env.testing") && fileBtn(".env.production"), "a skipped folder name is left out at every level");
    check(!q("#skipReset").disabled, "a changed list can be reset");
    await click("#skipReset");
    await until("the config folder back", () => fileBtn("app.yaml"));
    check(q("#skipReset").disabled && !q("#skipIn").value.includes("config"), "Reset restores the default list");
    q("#toast").className = "";
    check(!q("#themeBtn"), "the theme switch lives in the settings, not the header");
    // Updates: the settings show this build's version and the switch.
    check(text("#dlg").includes(`Version ${S.version}`), "the settings show the app's version");
    check(q("#updChk").checked, "update checks are on by default");
    await shot("15-settings");

    // The footer links open only this project's pages; the version opens its
    // release, or the release list for a development build, which has none.
    const opened = async (selector, url) => {
      await click(selector);
      await until(`the page behind ${selector}`, async () => (await call("openedPage")) === url);
    };
    const projectUrl = "https://github.com/nicostoe/open-sops-ui";
    check(q("#aboutVersion").innerText === `Version ${S.version}`, "the footer shows the version as a link");
    await opened("#aboutVersion", S.version.includes("-dev.") ? `${projectUrl}/releases` : `${projectUrl}/releases/tag/v${S.version}`);
    check(true, "the version opens its release page");
    await opened("#aboutRepo", projectUrl);
    check(true, "Source on GitHub opens the repository");
    await opened("#aboutIssues", `${projectUrl}/issues`);
    check(true, "Report a problem opens the issue list");
    check(q("#dlg").open, "opening a link keeps the settings open");
    const themeSel = q("#themeSel");
    themeSel.value = "dark";
    themeSel.dispatchEvent(new Event("change", { bubbles: true }));
    check(document.documentElement.dataset.theme === "dark", "choosing dark in the settings applies dark mode");
    await click("#dlg [data-close]");
    await click("#settingsBtn");
    await until("the settings", () => q("#dlg[open] #themeSel"));
    check(q("#themeSel").value === "dark", "the settings show the chosen theme when opened again");

    // A release list with a newer stable release, a newer pre-release and a
    // draft. Which one counts depends on this build: CI builds are pre-releases.
    check(q("#updateBtn").hidden, "no notice while there is nothing newer");
    const feed = [
      { tag_name: "v9.9.9", prerelease: false, draft: true, body: "draft" },
      { tag_name: "v9.0.0-beta.1", prerelease: true, draft: false, body: "### Features\n\n- Faster startup\n- <img src=x onerror=alert(1)>" },
      { tag_name: "v1.2.0", prerelease: false, draft: false, body: "### Features\n\n- Faster startup ([abc1234](https://example.invalid))\n\n**Full changelog:** https://example.invalid" },
    ];
    await call("setReleases", { releases: feed });
    const onPrerelease = S.version.includes("-");
    const expected = onPrerelease ? "v9.0.0-beta.1" : "v1.2.0";
    q("#updChk").click();
    await until("update checks off", () => S.settings.checkUpdates === false);
    check(q("#updateBtn").hidden, "turning update checks off hides the notice");
    q("#updChk").click();
    await until("update checks on", () => S.settings.checkUpdates === true);
    await until("the update notice", () => !q("#updateBtn").hidden);
    await click("#dlg [data-close]");
    check(q("#updateBtn").title.includes(expected.slice(1)),
      onPrerelease ? "a pre-release build hears about the newest release, pre-releases included"
        : "a stable build hears about the newest stable release, not the pre-release");
    await click("#updateBtn");
    await until("the update dialog", () => q("#dlg[open] #updOpen"));
    check(text("#dlg").includes(`You have ${S.version}`) && text("#dlg").includes("Faster startup"), "the dialog shows both versions and the release notes");
    check(!q("#dlg .notes img"), "release notes are shown as text, not as HTML");
    q("#toast").className = "";
    await shot("16-update-available");
    await click("#updOpen");
    await until("the release page", async () =>
      (await call("openedPage")) === `https://github.com/nicostoe/open-sops-ui/releases/tag/${expected}`);
    check(!q("#dlg").open, "the release page opens for exactly the announced release");

    // GitHub unreachable: the app keeps working, a notice already shown stays,
    // automatic checks stay quiet, and "Check now" says what happened.
    const toastShown = () => q("#toast").classList.contains("show");
    q("#toast").className = "";
    await call("setReleases", { releases: { simulate: "error" } });
    await checkForUpdate();
    check(!q("#updateBtn").hidden, "a failed check keeps the notice that was already shown");
    check(!toastShown(), "an automatic check that fails says nothing");
    await click("#settingsBtn");
    await until("the settings", () => q("#dlg[open] #updNow"));
    await click("#updNow");
    await shows("#toast", "Couldn't reach GitHub");
    check(true, "Check now reports a failed check");
    q("#toast").className = "";

    // A network that never answers: the check gives up after its time limit.
    await call("setReleases", { releases: { simulate: "hang", timeoutMs: 1500 } });
    const started = Date.now();
    await click("#updNow");
    const busy = Date.now();
    await api("state");
    check(Date.now() - busy < 1000, "the app keeps responding while a check hangs");
    await click("#updNow"); // asking again while it runs waits for the same check
    await until("the timeout message", () => toastShown() && text("#toast").includes("Couldn't reach GitHub"), 8000);
    check(Date.now() - started >= 1400, "a hanging check gives up after its time limit, not before");
    q("#toast").className = "";
    await click("#dlg [data-close]");

    // Back to nothing newer: the notice goes away on the next check.
    await call("setReleases", { releases: [] });
    await checkForUpdate();
    await until("the notice gone", () => q("#updateBtn").hidden);
    await shot("17-dark");
    await click("#keysBtn");
    await until("the key in the dialog", () => q("#dlg [data-copy]"));
    await shot("18-dark-keys");
    await click("#dlg [data-close]");
    await click("#changesBtn");
    await until("the changes", () => q("#dlg[open] .chg"));
    await shot("19-dark-changes");
    await click("#dlg [data-close]");
    fileBtn("app.yaml").click();
    await until("the YAML file", () => cur && cur.path.endsWith("app.yaml") && q("#editor #textEdit"));
    await click("#revealText");
    await shot("19a-dark-text-file");

    await call("done", { ok: true, log });
  } catch (e) {
    log.push(`FAILED ${e.message}`);
    try {
      await shot("99-failure");
    } catch { /* report without it */ }
    await call("done", { ok: false, error: e.message, log });
  }
})();
