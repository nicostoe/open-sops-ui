// End-to-end UI test: drives the real app window through WebDriver and checks
// the results on disk with sops. No dependencies beyond Node 20.
//
// Usage: node ui-tests/run.mjs <path-to-app-binary>
// Needs on PATH: tauri-driver, a native WebDriver (WebKitWebDriver on Linux,
// msedgedriver on Windows) and sops. macOS has no WebDriver for WKWebView.
// Everything runs in a temp folder; the real keys.txt is never touched.
import { execFileSync, spawn } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

const appBinary = resolve(process.argv[2] ?? "");
if (!existsSync(appBinary)) {
  console.error("usage: node ui-tests/run.mjs <path-to-app-binary>");
  process.exit(2);
}

const PORT = 4444;
const BASE = `http://127.0.0.1:${PORT}`;
const ELEMENT = "element-6066-11e4-a52e-4f735466cecf";
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// ---- test fixture ---------------------------------------------------------
const root = mkdtempSync(join(tmpdir(), "sops-env-ui-"));
const project = join(root, "project");
const config = join(root, "config");
const keysFile = join(config, "sops", "age", "keys.txt");
const envFile = join(project, ".env");
const ORIGINAL = "DATABASE_URL=postgres://user:pw@db/app\n# payment\nSTRIPE_SECRET=sk_test_1\n";
mkdirSync(project, { recursive: true });
mkdirSync(config, { recursive: true });
writeFileSync(envFile, ORIGINAL);

// The app and sops read their config from here instead of the real profile.
const env = { ...process.env, XDG_CONFIG_HOME: config, AppData: config, SOPS_AGE_KEY_FILE: keysFile };
delete env.SOPS_AGE_KEY;

const decrypt = () =>
  execFileSync(process.env.SOPS_BIN ?? "sops", ["decrypt", "--input-type", "dotenv", "--output-type", "dotenv", envFile], {
    env, encoding: "utf8",
  }).replace(/\r\n/g, "\n");

// ---- WebDriver client -----------------------------------------------------
let session = null;

async function wd(method, path, body) {
  const res = await fetch(`${BASE}${session ? `/session/${session}` : ""}${path}`, {
    method,
    headers: { "content-type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const data = await res.json().catch(() => ({}));
  if (!res.ok) throw new Error(`${method} ${path}: ${data?.value?.message ?? res.status}`);
  return data.value;
}

const js = (script, ...args) => wd("POST", "/execute/sync", { script, args });

async function until(what, probe, timeout = 15000) {
  const end = Date.now() + timeout;
  let last;
  while (Date.now() < end) {
    try {
      const value = await probe();
      if (value) return value;
    } catch (e) {
      last = e;
    }
    await sleep(150);
  }
  throw new Error(`timed out waiting for ${what}${last ? `: ${last.message}` : ""}`);
}

const find = (selector) =>
  until(`element ${selector}`, async () => (await wd("POST", "/element", { using: "css selector", value: selector }))[ELEMENT]);

async function click(selector) {
  // A scripted click is not affected by scrolling or overlays in the web view.
  await find(selector);
  await js("document.querySelector(arguments[0]).click()", selector);
}

async function type(selector, text) {
  const id = await find(selector);
  await wd("POST", `/element/${id}/clear`, {});
  await wd("POST", `/element/${id}/value`, { text });
}

const text = (selector) => js("return document.querySelector(arguments[0])?.innerText ?? ''", selector);

function check(condition, message) {
  if (!condition) throw new Error(`check failed: ${message}`);
  console.log(`  ok  ${message}`);
}

// ---- the test -------------------------------------------------------------
async function run() {
  await until("the WebDriver server", async () => (await fetch(`${BASE}/status`)).ok, 20000);
  const created = await wd("POST", "/session", {
    capabilities: { alwaysMatch: { browserName: "wry", "tauri:options": { application: appBinary, args: [project] } } },
  });
  session = created.sessionId;

  console.log("start");
  await find("#files .file[data-i]");
  check((await text("#files")).includes(".env"), "the plaintext file is listed");
  check((await text(".sopsinfo")).startsWith("sops "), "the app found sops");
  check((await js("return document.querySelector('#keysDot').className")) === "dot", "a missing key is flagged");

  console.log("create a key");
  await click("#keysBtn");
  await click("#dlg [data-f=new]");
  await click("#dlg #kp");
  await until("keys.txt on disk", () => existsSync(keysFile));
  check(readFileSync(keysFile, "utf8").includes("AGE-SECRET-KEY-"), "the new key is saved to the default location");
  await until("the key in the dialog", async () => (await text("#dlg")).includes("age1"));
  await click("#dlg [data-close]");

  console.log("encrypt the file");
  await click("#files .file[data-i]");
  await find("#editor .row");
  check((await js("return document.querySelectorAll('#editor .row').length")) === 2, "both variables are shown");
  check((await text(".filehead")).includes("Not encrypted yet"), "the file is shown as plaintext");
  await click("#save");
  await click("#dlg #go");
  await until("the encrypted file", () => readFileSync(envFile, "utf8").includes("sops_version="));
  check(!readFileSync(envFile, "utf8").includes("sk_test_1"), "the secret is no longer readable on disk");
  check(decrypt() === ORIGINAL, "sops decrypts it back to the original, comment included");
  await until("the recipient count", async () => (await text(".filehead")).includes("1 recipient"));

  console.log("edit a value");
  const before = readFileSync(envFile, "utf8");
  await type("#editor .row:nth-of-type(2) .val", "sk_test_changed");
  await until("the unsaved marker", async () => (await text("#dirty")).includes("Unsaved"));
  await click("#save");
  await until("the saved change", () => decrypt().includes("STRIPE_SECRET=sk_test_changed"));
  const line = (content, key) => content.split("\n").find((l) => l.startsWith(key));
  check(line(before, "DATABASE_URL=") === line(readFileSync(envFile, "utf8"), "DATABASE_URL="), "the untouched value keeps its ciphertext");
  await until("the saved marker", async () => (await text("#dirty")).includes("Saved"));

  console.log("add a variable");
  await click("#addVar");
  await type("#editor .row:nth-of-type(3) .k", "NEW_VAR");
  await type("#editor .row:nth-of-type(3) .val", "hello world");
  await click("#save");
  await until("the new variable", () => decrypt().includes("NEW_VAR=hello world"));
  check(true, "a new variable is saved");

  console.log("dialogs and theme");
  await click("#recBtn");
  await until("the recipients dialog", async () => (await text("#dlg")).includes("You (default)"));
  check(true, "the recipients dialog lists your key");
  await click("#dlg [data-close]");
  const themes = new Set();
  for (let i = 0; i < 3; i++) {
    await click("#themeBtn");
    themes.add(await js("return document.documentElement.dataset.theme"));
  }
  check(themes.has("light") && themes.has("dark"), "the theme switches between light and dark");
}

const driver = spawn("tauri-driver", ["--port", String(PORT)], { env, stdio: ["ignore", "ignore", "inherit"] });
driver.on("error", () => {
  console.error("tauri-driver isn't installed. Install it with: cargo install tauri-driver --locked");
  process.exit(2);
});

let failed = false;
try {
  await run();
  console.log("UI test passed");
} catch (e) {
  failed = true;
  console.error(`UI test failed: ${e.message}`);
  try {
    console.error(`toast: ${await text("#toast")}`);
  } catch { /* no session */ }
} finally {
  if (session) await wd("DELETE", "", undefined).catch(() => {});
  driver.kill();
  rmSync(root, { recursive: true, force: true });
}
process.exit(failed ? 1 : 0);
