// Runs the app's scripted UI self-test and checks the result on disk.
//
// The app clicks through itself in its real window (see ui/selftest.js),
// which works on Linux, Windows and macOS. This script prepares a throwaway
// project and config folder, starts the app in test mode, then verifies the
// files with sops and collects the screenshots.
//
// Usage: node ui-tests/selftest.mjs <app-binary> [output-folder] [--require-screenshots]
// Needs a display (Xvfb is enough) and, on Linux, ImageMagick for screenshots.
// The real keys.txt and config are never touched.
import { execFileSync, spawn } from "node:child_process";
import { generateKeyPairSync } from "node:crypto";
import { existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

const args = process.argv.slice(2);
const requireShots = args.includes("--require-screenshots");
const [binaryArg, outArg] = args.filter((a) => !a.startsWith("--"));
const appBinary = resolve(binaryArg ?? "");
if (!binaryArg || !existsSync(appBinary)) {
  console.error("usage: node ui-tests/selftest.mjs <app-binary> [output-folder] [--require-screenshots]");
  process.exit(2);
}
const out = resolve(outArg ?? "ui-test-output");
mkdirSync(out, { recursive: true });
// Empty it instead of removing it: the folder may be a mount point.
for (const entry of readdirSync(out)) rmSync(join(out, entry), { recursive: true, force: true });

// The release list the app sees instead of GitHub's. Empty to start with, so
// the screenshots show no update notice; ui/selftest.js fills it for its step.
writeFileSync(join(out, "releases.json"), "[]");

// A small, realistic project: the screenshots double as the README's pictures.
const ORIGINAL = [
  "DATABASE_URL=postgres://shop:s3cr3t-pw@db.internal:5432/shop",
  "# payment",
  "STRIPE_SECRET=sk_test_1",
  "JWT_SIGNING_KEY=6f1e0c9a4b7d2e8f5a3c1b9d",
  "SMTP_PASSWORD=mail-pass-1234",
  "",
].join("\n");
const EXPECTED = ORIGINAL.replace("sk_test_1", "sk_test_changed") + "SENTRY_DSN=https://abc123@sentry.example/42\n";
// A plain text file, which sops encrypts as one block of text.
const NOTES = "Recovery codes for the admin account\n7f3a-91c2-e5d0\n2b8e-40d7-a913\n";
const NOTES_EXPECTED = NOTES + "c64f-1e8b-77a2\n";
const EXPECTED_SHOTS = 31;
const TIMEOUT_MS = 180_000;

// An age key pair, as age-keygen makes it: X25519 keys in bech32. CI runners
// have no age-keygen, and the test needs a key before the app runs.
function ageKeyPair() {
  const CHARSET = "qpzry9x8gf2tvdw0s3jn54khce6mua7l";
  const polymod = (values) => {
    const G = [0x3b6a57b2, 0x26508e6d, 0x1ea119fa, 0x3d4233dd, 0x2a1462b3];
    let chk = 1;
    for (const v of values) {
      const top = chk >>> 25;
      chk = ((chk & 0x1ffffff) << 5) ^ v;
      for (let i = 0; i < 5; i++) if ((top >>> i) & 1) chk ^= G[i];
    }
    return chk >>> 0;
  };
  const bech32 = (hrp, bytes) => {
    const words = [];
    let acc = 0, bits = 0;
    for (const b of bytes) {
      acc = ((acc << 8) | b) & 0xffff; bits += 8;
      while (bits >= 5) { bits -= 5; words.push((acc >>> bits) & 31); }
    }
    if (bits > 0) words.push((acc << (5 - bits)) & 31);
    const codes = [...hrp].map((c) => c.charCodeAt(0));
    const pm = polymod([...codes.map((c) => c >>> 5), 0, ...codes.map((c) => c & 31), ...words, 0, 0, 0, 0, 0, 0]) ^ 1;
    return `${hrp}1${[...words, ...[0, 1, 2, 3, 4, 5].map((i) => (pm >>> (5 * (5 - i))) & 31)].map((w) => CHARSET[w]).join("")}`;
  };
  const { publicKey, privateKey } = generateKeyPairSync("x25519");
  return {
    secret: bech32("age-secret-key-", Buffer.from(privateKey.export({ format: "jwk" }).d, "base64url")).toUpperCase(),
    public: bech32("age", Buffer.from(publicKey.export({ format: "jwk" }).x, "base64url")),
  };
}

// OPEN_SOPS_UI_SELFTEST_ROOT pins the folder, so screenshots show a tidy path
// instead of a random temp name. It must not exist yet.
const fixedRoot = process.env.OPEN_SOPS_UI_SELFTEST_ROOT;
if (fixedRoot && existsSync(fixedRoot)) {
  console.error(`${fixedRoot} already exists; OPEN_SOPS_UI_SELFTEST_ROOT must point to a new folder`);
  process.exit(2);
}
const root = fixedRoot ? resolve(fixedRoot) : mkdtempSync(join(tmpdir(), "open-sops-ui-test-"));
const project = join(root, "projects", "shop-api");
const config = join(root, ".config");
const keysFile = join(config, "sops", "age", "keys.txt");
const envFile = join(project, ".env.production");
const notesFile = join(project, "config", "recovery-codes.txt");
// Holds the teammate's key for the app to paste; removed once the app is done.
const fixtureFile = join(out, "fixture.json");
mkdirSync(project, { recursive: true });
mkdirSync(config, { recursive: true });
// A second, previously opened project for the recent-folders list.
const blog = join(root, "projects", "blog");
mkdirSync(blog, { recursive: true });
writeFileSync(join(blog, ".env"), "SITE_URL=https://blog.example\n");
mkdirSync(join(config, "open-sops-ui"), { recursive: true });
writeFileSync(join(config, "open-sops-ui", "config.json"), JSON.stringify({ recent: [blog] }));

// The app, its web view and sops keep their data here instead of the real profile.
const env = { ...process.env, XDG_CONFIG_HOME: config, XDG_DATA_HOME: join(root, "data") };
// Windows variable names ignore case; drop the inherited spelling so ours is the only one.
for (const name of Object.keys(env)) if (name.toLowerCase() === "appdata") delete env[name];
env.APPDATA = config;
delete env.SOPS_AGE_KEY;
delete env.SOPS_AGE_KEY_FILE;

const failures = [];
const check = (condition, message) => {
  console.log(`  ${condition ? "ok  " : "FAIL"} ${message}`);
  if (!condition) failures.push(message);
};

function runApp(extraArgs, extraEnv = {}) {
  return new Promise((done) => {
    const child = spawn(appBinary, extraArgs, { env: { ...env, ...extraEnv }, stdio: ["ignore", "ignore", "inherit"] });
    const timer = setTimeout(() => {
      child.kill("SIGKILL");
      done("timeout");
    }, TIMEOUT_MS);
    child.on("error", (e) => { clearTimeout(timer); done(`could not start: ${e.message}`); });
    child.on("exit", (code) => { clearTimeout(timer); done(code); });
  });
}

try {
  // Ask the app which sops it uses, so the checks below use the same one.
  const reportFile = join(root, "self-check.json");
  await runApp(["--self-check", reportFile]);
  const report = existsSync(reportFile) ? JSON.parse(readFileSync(reportFile, "utf8")) : {};
  const sops = report.sops?.path;
  console.log(`app ${report.appVersion ?? "?"}, sops ${report.sops?.version ?? "not found"} (${report.sops?.source ?? "-"})`);
  if (!sops) throw new Error("the app found no sops binary");

  // The project's files are encrypted for a teammate's key, which the test
  // pastes in the app. Encrypted the way sops would on the command line.
  const team = ageKeyPair();
  const encrypt = (name, plain, type = "dotenv", stored = type) =>
    execFileSync(sops, ["encrypt", "--age", team.public, "--input-type", type, "--output-type", stored, "--filename-override", name],
      { input: plain, cwd: root, env, encoding: "utf8" });
  const put = (rel, content) => {
    const file = join(project, ...rel.split("/"));
    mkdirSync(join(file, ".."), { recursive: true });
    writeFileSync(file, content);
  };
  put(".env.production", encrypt(".env.production", ORIGINAL));
  put("services/worker/config/.env.testing", encrypt(".env.testing", "QUEUE_URL=redis://localhost:6379/9\n"));
  put("services/worker/.env", encrypt(".env", "QUEUE_URL=redis://localhost:6379/1\nCONCURRENCY=4\n"));
  put(".env.staging", encrypt(".env.staging", "DATABASE_URL=postgres://shop:dev@localhost:5432/shop_staging\nLOG_LEVEL=debug\n"));
  put("config/recovery-codes.txt", encrypt("recovery-codes.txt", NOTES, "binary", "json"));
  put("config/app.yaml", encrypt("app.yaml", "server:\n  port: 8080\n  session_secret: 9c1f4e2a7b\nmail:\n  host: smtp.example\n  password: m41l-pw\n", "yaml"));
  put("config/feature-flags.json", encrypt("feature-flags.json", JSON.stringify({ checkout: { enabled: true, apiKey: "ff_live_51c0" } }, null, 2), "json"));
  put("config/billing.ini", encrypt("billing.ini", "[stripe]\nsecret = sk_live_6a2e\nwebhook = whsec_8d41\n\n[invoice]\nprefix = SHOP\n", "ini"));
  // Plaintext files aren't the app's business: neither listed nor opened.
  put(".env.example", "DATABASE_URL=\nSTRIPE_SECRET=\nJWT_SIGNING_KEY=\nSMTP_PASSWORD=\n");
  put(".gitignore", "/.env.staging\n");
  writeFileSync(fixtureFile, JSON.stringify({
    teamKey: team.secret,
    // Files the test drops into the project while the app runs, like a git pull.
    preview: encrypt(".env.preview", "PREVIEW_URL=https://preview.example\n"),
    qa: encrypt(".env.qa", "QA=1\n"),
  }));

  // The project is a Git repository, the usual home of these files: the
  // example, the text file and the other formats are committed, the staging file
  // is ignored, the rest is untracked.
  const git = (...args) =>
    execFileSync("git", ["-c", "user.name=UI test", "-c", "user.email=ui-test@example.invalid", "-c", "commit.gpgsign=false", "-c", "core.autocrlf=false", ...args], { cwd: project, stdio: "ignore" });
  try {
    git("init", "-q", "-b", "main");
    git("add", ".gitignore", ".env.example", "config");
    git("commit", "-q", "-m", "Add the example env file and the config");
  } catch {
    throw new Error("the UI test needs git on PATH (the app itself works without it)");
  }

  console.log("running the scripted UI test in the app window");
  const code = await runApp([project], { OPEN_SOPS_UI_SELFTEST: out });
  rmSync(fixtureFile, { force: true });
  const resultFile = join(out, "result.json");
  const result = existsSync(resultFile) ? JSON.parse(readFileSync(resultFile, "utf8")) : null;
  for (const line of result?.log ?? []) console.log(`  app: ${line}`);
  if (code === "timeout") throw new Error("the app didn't finish the test in time");
  if (!result) throw new Error(`the app exited with ${code} and wrote no result`);
  check(result.ok === true, result.ok ? "the app completed every UI step" : `UI steps: ${result.error}`);

  console.log("checking the files on disk");
  check(existsSync(keysFile) && readFileSync(keysFile, "utf8").includes("AGE-SECRET-KEY-"), "the created key is in keys.txt");
  const onDisk = readFileSync(envFile, "utf8");
  check(onDisk.includes("sops_version="), "the env file is encrypted");
  check(!onDisk.includes("sk_test"), "no secret value is readable in the file");
  let decrypted = "";
  try {
    decrypted = execFileSync(sops, ["decrypt", "--input-type", "dotenv", "--output-type", "dotenv", envFile], {
      env: { ...env, SOPS_AGE_KEY_FILE: keysFile }, encoding: "utf8",
    }).replace(/\r\n/g, "\n");
  } catch (e) {
    failures.push(`sops couldn't decrypt the file: ${e.message.split("\n")[0]}`);
  }
  check(decrypted === EXPECTED, "sops decrypts it to the edited content, comment included");
  let created = "";
  try {
    created = execFileSync(sops, ["decrypt", "--input-type", "dotenv", "--output-type", "dotenv", join(project, ".env.ci")], {
      env: { ...env, SOPS_AGE_KEY_FILE: keysFile }, encoding: "utf8",
    }).replace(/\r\n/g, "\n");
  } catch (e) {
    failures.push(`sops couldn't decrypt the new file: ${e.message.split("\n")[0]}`);
  }
  check(created === "CI_TOKEN=abc123\n", "the file created in the app decrypts to what was entered");
  let notes = "";
  try {
    notes = execFileSync(sops, ["decrypt", "--input-type", "json", "--output-type", "binary", notesFile], {
      env: { ...env, SOPS_AGE_KEY: team.secret }, encoding: "utf8",
    });
  } catch (e) {
    failures.push(`sops couldn't decrypt the text file: ${e.message.split("\n")[0]}`);
  }
  check(notes === NOTES_EXPECTED, "the text file edited in the app decrypts to the edited content");
  check(!readFileSync(notesFile, "utf8").includes("c64f-1e8b"), "the edited text file is encrypted on disk");

  const shots = readdirSync(out).filter((f) => f.endsWith(".png")).sort();
  const usable = shots.filter((f) => statSync(join(out, f)).size > 5000);
  console.log(`screenshots: ${usable.length} of ${EXPECTED_SHOTS} in ${out}`);
  if (requireShots) check(usable.length >= EXPECTED_SHOTS, `all ${EXPECTED_SHOTS} screenshots were taken`);
} catch (e) {
  failures.push(e.message);
  console.error(`  FAIL ${e.message}`);
} finally {
  rmSync(root, { recursive: true, force: true });
  rmSync(fixtureFile, { force: true });
}

console.log(failures.length ? `UI self-test failed (${failures.length})` : "UI self-test passed");
process.exit(failures.length ? 1 : 0);
