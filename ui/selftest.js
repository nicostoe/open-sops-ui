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
  function setTheme(name) {
    localStorage.setItem("theme", name);
    applyTheme();
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
    await click("#save");
    await shows("#dirty", "Saved");
    check(document.querySelectorAll("#editor .row").length === 5, "five variables after saving");

    await until("the modified mark", () => fileBtn(".env.production").querySelector(".gs")?.innerText === "M");
    await click("#changesBtn");
    await until("the changes", () => q("#dlg[open] .chg")); // [open]: a closed dialog keeps its old content
    check(kinds().join(", ") === "Changed STRIPE_SECRET, Added SENTRY_DSN", "the comparison lists exactly what changed since the commit");
    check(text("#dlg").includes("3 unchanged"), "unchanged variables are counted, not listed");
    await shot("08-changes-since-commit");
    await click("#chgReveal");
    await shows("#dlg", "sk_test_changed");
    check(text("#dlg").includes("sk_test_1"), "revealing shows the old and the new value");
    await click("#dlg [data-close]");

    await click("#recBtn");
    await shows("#dlg", "You (default)");
    await shot("09-recipients");
    await click("#dlg [data-close]");

    await type("#editor .row:nth-of-type(1) .val", "unsaved");
    await click("#folderBtn");
    await shows("#dlg2", "Discard unsaved changes?");
    await shot("10-confirm");
    await click("#dlg2 #no");
    await click("#revert");
    check(!text("#dirty").includes("Unsaved"), "discarding restores the saved values");

    await click("#folderBtn");
    await shows("#dlg", "blog");
    check(document.querySelectorAll("#dlg [data-r]").length === 1, "the other recently opened folder is offered");
    await shot("11-recent-folders");
    await click("#dlg [data-close]");

    setTheme("dark");
    check(document.documentElement.dataset.theme === "dark", "dark mode is applied");
    await shot("12-dark");
    await click("#keysBtn");
    await until("the key in the dialog", () => q("#dlg [data-copy]"));
    await shot("13-dark-keys");
    await click("#dlg [data-close]");
    await click("#changesBtn");
    await until("the changes", () => q("#dlg[open] .chg"));
    await shot("14-dark-changes");
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
