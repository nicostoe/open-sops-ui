// Scripted UI self-test. app.js loads this file only when the app was started
// with SOPS_ENV_UI_TEST set; it never runs for a normal user.
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

    await click("#files .file[data-i]");
    await until("the variables", () => q("#editor .row"));
    check(document.querySelectorAll("#editor .row").length === 4, "all four variables are shown");
    check(text(".filehead").includes("Not encrypted yet"), "the file is shown as plaintext");
    await shot("04-plaintext-file");

    await click("#save");
    await until("the recipient chooser", () => q("#dlg #go"));
    await shot("05-encrypt-for");
    await click("#dlg #go");
    await shows(".filehead", "1 recipient");
    check(q("#files .file").classList.contains("s-ok"), "the file is marked as encrypted and readable");
    await shot("06-encrypted");

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

    await click("#recBtn");
    await shows("#dlg", "You (default)");
    await shot("08-recipients");
    await click("#dlg [data-close]");

    await type("#editor .row:nth-of-type(1) .val", "unsaved");
    await click("#folderBtn");
    await shows("#dlg2", "Discard unsaved changes?");
    await shot("09-confirm");
    await click("#dlg2 #no");
    await click("#revert");
    check(!text("#dirty").includes("Unsaved"), "discarding restores the saved values");

    setTheme("dark");
    check(document.documentElement.dataset.theme === "dark", "dark mode is applied");
    await shot("10-dark");
    await click("#keysBtn");
    await until("the key in the dialog", () => q("#dlg [data-copy]"));
    await shot("11-dark-keys");
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
