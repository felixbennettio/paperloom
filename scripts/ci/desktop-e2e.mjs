// Drives the installed app through W3C WebDriver (Linux: tauri-driver and
// WebKitWebDriver; Windows: msedgedriver attached to the running app) with
// its real library, files and network: adds a paper by DOI, adds a scanned
// PDF (its DOI read from the page's picture), opens it in the reader,
// recognizes its text, finds a word only the picture holds, writes a note,
// cites the paper in a Word document and opens a reference file with the
// running app, as a file manager's Open With does. File dialogs are answered
// with xdotool (GTK, Linux) or UI Automation (windows-dialog.ps1). Each step
// keeps a screenshot and the page's text; summary.txt lists what failed.
// desktop-e2e.sh (Linux) and windows-e2e.ps1 prepare the machine.
//
// E2E_STEPS (names, comma-separated) runs only those steps. Two run only when
// named there, from wps-e2e.sh with WPS for Linux installed: "Install the WPS
// add-in" (from Settings) and "Cite in WPS" (WPS opens WPS_DOCUMENT with the
// add-in, whose task pane pane-e2e.js walks; WPS_REPORTS is where the probe
// server keeps what it reports).
//
//   node scripts/ci/desktop-e2e.mjs <app binary> <out folder> <files folder>
//
// <files folder> holds scan.pdf, paper.docx and curie.ris (e2e-files.py); the
// app keeps its data under APP_HOME.
import { execFileSync, spawn } from "node:child_process";
import {
  appendFileSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  openSync,
  readdirSync,
  readFileSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const [app, out, files] = process.argv.slice(2);
if (!app || !out || !files) throw new Error("usage: desktop-e2e.mjs <app binary> <out> <files>");
const DRIVER = process.env.WEBDRIVER_URL ?? "http://127.0.0.1:4444";
const APP_HOME = process.env.APP_HOME ?? "";
const TITLE = "Nanometre-scale thermometry in a living cell";
const ENTER = "\uE007";
const WINDOWS = process.platform === "win32";
const HERE = dirname(fileURLToPath(import.meta.url));
mkdirSync(out, { recursive: true });

const failed = [];
const warned = [];
const fail = (what) => {
  console.error(`FAILED: ${what}`);
  failed.push(what);
};
const warn = (what) => {
  console.error(`WARNING: ${what}`);
  warned.push(what);
};
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

async function wd(method, path, body) {
  const response = await fetch(`${DRIVER}${path}`, {
    method,
    headers: { "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const json = await response.json().catch(() => ({}));
  if (!response.ok) throw new Error(`${method} ${path}: ${json.value?.error ?? response.status} ${json.value?.message ?? ""}`.trim());
  return json.value;
}

let session = "";
const at = (path) => `/session/${session}${path}`;
const run = (script, args = []) => wd("POST", at("/execute/sync"), { script, args });
const pageText = () => run("return document.body ? document.body.innerText : ''");

/** The first element at an XPath, waiting for it to appear. */
async function find(xpath, timeout = 15_000) {
  const until = Date.now() + timeout;
  for (;;) {
    try {
      const element = await wd("POST", at("/element"), { using: "xpath", value: xpath });
      return Object.values(element)[0];
    } catch (e) {
      if (Date.now() > until) throw new Error(`not found: ${xpath} (${e.message})`);
      await sleep(500);
    }
  }
}

async function click(xpath, timeout) {
  const id = await find(xpath, timeout);
  await wd("POST", at(`/element/${id}/click`), {});
  await sleep(700);
}

async function type(xpath, text) {
  const id = await find(xpath);
  await wd("POST", at(`/element/${id}/value`), { text });
  await sleep(500);
}

/** Waits until the page's text matches; returns that text. */
async function waitFor(pattern, timeout = 30_000) {
  const until = Date.now() + timeout;
  let text = "";
  while (Date.now() < until) {
    text = await pageText().catch(() => "");
    if (pattern.test(text)) return text;
    await sleep(1000);
  }
  throw new Error(`no ${pattern} on the page within ${timeout / 1000} s`);
}

const button = (label) =>
  `//button[@aria-label=${quote(label)} or @title=${quote(label)} or normalize-space()=${quote(label)}]`;
// The activity bar's buttons, apart from same-named ones in the page.
const nav = (label) => `//nav[contains(@class, "activity-bar")]//button[@aria-label=${quote(label)}]`;
const menuItem = (label) => `//button[(@role="menuitem" or @role="menuitemcheckbox") and @title=${quote(label)}]`;
function quote(text) {
  if (!text.includes('"')) return `"${text}"`;
  return `concat(${text.split('"').map((part) => `"${part}"`).join(`, '"', `)})`;
}

let shots = 0;
async function shot(name) {
  shots++;
  const file = join(out, `${String(shots).padStart(2, "0")}-${name}`);
  try {
    writeFileSync(`${file}.png`, Buffer.from(await wd("GET", at("/screenshot")), "base64"));
    writeFileSync(`${file}.txt`, await pageText());
  } catch (e) {
    warn(`screenshot ${name}: ${e.message}`);
  }
}

let dialogs = 0;
const answered = new Set();
/** Answers a GTK file dialog: types the path into an Open dialog, or takes
 * the name a Save dialog suggests. */
function answerDialog(title, path) {
  if (WINDOWS) return answerWindowsDialog(title, path);
  const xdotool = (...args) => execFileSync("xdotool", args, { encoding: "utf8", timeout: 20_000 }).trim();
  const sleep = (seconds) => execFileSync("sleep", [String(seconds)]);
  const visible = () => {
    try {
      return xdotool("search", "--onlyvisible", "--name", `^${title}$`).split("\n").filter(Boolean);
    } catch {
      return [];
    }
  };
  // This dialog, not one an earlier step left open.
  const until = Date.now() + 30_000;
  let window = "";
  while (!window && Date.now() < until) {
    window = visible().find((id) => !answered.has(id)) ?? "";
    if (!window) sleep(1);
  }
  if (!window) {
    screen(`no-${title}`);
    throw new Error(`no "${title}" dialog`);
  }
  answered.add(window);
  const open = () => visible().includes(window);
  // No window manager gives it the keyboard: click its top edge (the
  // location field or folder buttons, which the typed path makes moot),
  // then focus it.
  const focus = () => {
    const size = Object.fromEntries(xdotool("getwindowgeometry", "--shell", window).split("\n").map((line) => line.split("=")));
    xdotool("mousemove", "--window", window, String(Math.round(Number(size.WIDTH ?? 400) / 2)), "14");
    xdotool("click", "1");
    xdotool("windowfocus", window);
    sleep(1);
  };
  focus();
  if (path) {
    // Ctrl+L brings up the location field, which takes a moment: keys typed
    // before it is there are lost ("/tm" went missing), so the path is
    // typed again over the first try.
    xdotool("key", "ctrl+l");
    sleep(1);
    for (let round = 0; round < 2; round++) {
      xdotool("key", "ctrl+a");
      xdotool("type", "--delay", "40", path);
      sleep(1);
    }
  }
  dialogs++;
  screen(`dialog-${dialogs}-${title}`);
  // A completion list may take the first Return.
  for (let tries = 0; tries < 3 && open(); tries++) {
    xdotool("key", "Return");
    sleep(2);
  }
  if (open()) {
    screen(`${title}-stayed-open`);
    // Closed, so the next steps are not behind it.
    focus();
    xdotool("key", "Escape");
    throw new Error(`the "${title}" dialog stayed open`);
  }
}

/** Windows names its dialogs Open and Save As. */
function answerWindowsDialog(title, path) {
  const name = { "Open File": "Open", "Save File": "Save As" }[title] ?? title;
  dialogs++;
  const shot = join(out, `screen-dialog-${dialogs}-${name.toLowerCase().replace(/[^a-z0-9]+/g, "-")}.png`);
  const args = ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", join(HERE, "windows-dialog.ps1"), "-Title", name, "-Shot", shot];
  if (path) args.push("-Path", path);
  // The script's whole output is kept beside its screenshot.
  const log = shot.replace(/\.png$/, ".log");
  try {
    const said = execFileSync("powershell", args, { encoding: "utf8", timeout: 90_000, stdio: ["ignore", "pipe", "pipe"] });
    writeFileSync(log, said);
    console.log(`    ${said.trim().split("\n").pop()}`);
  } catch (e) {
    writeFileSync(log, `${e.stdout ?? ""}\n${e.stderr ?? ""}`);
    throw new Error(`${name} dialog: ${(e.stderr || e.message).toString().trim().split("\n").slice(0, 3).join(" ")}`);
  }
}

/** The whole display, dialogs included (WebDriver sees only the page). */
function screen(name) {
  if (WINDOWS) return;
  try {
    execFileSync("import", ["-window", "root", join(out, `screen-${name.toLowerCase().replace(/[^a-z0-9]+/g, "-")}.png`)], { timeout: 20_000 });
  } catch (e) {
    warn(`screen ${name}: ${e.message}`);
  }
}

const ONLY = process.env.E2E_STEPS?.split(",").map((name) => name.trim());
const OPTIONAL = new Set(["Install the WPS add-in", "Cite in WPS"]);
// Only the Windows app installs the Word add-in.
if (!WINDOWS) OPTIONAL.add("Install the Word add-in");

async function step(name, body) {
  if (ONLY ? !ONLY.includes(name) : OPTIONAL.has(name)) return;
  console.log(`==> ${name}`);
  const started = Date.now();
  try {
    await body();
  } catch (e) {
    fail(`${name}: ${e.message}`);
    await shot(`${name.toLowerCase().replace(/[^a-z0-9]+/g, "-")}-failed`);
    // An open dialog or menu would stand in the next step's way.
    await wd("POST", at("/actions"), {
      actions: [{ type: "key", id: "keyboard", actions: [{ type: "keyDown", value: "\uE00C" }, { type: "keyUp", value: "\uE00C" }] }],
    }).catch(() => undefined);
  }
  console.log(`    ${Math.round((Date.now() - started) / 1000)} s`);
}

function filesUnder(dir) {
  const found = [];
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) found.push(...filesUnder(path));
    else found.push(path);
  }
  return found;
}

// tauri-driver starts the app; WEBDRIVER_CAPABILITIES may instead attach to
// one already running (Windows: msedgedriver's debuggerAddress).
const capabilities = process.env.WEBDRIVER_CAPABILITIES
  ? JSON.parse(process.env.WEBDRIVER_CAPABILITIES)
  : { "tauri:options": { application: app } };
session = (await wd("POST", "/session", { capabilities: { alwaysMatch: capabilities } })).sessionId;
let joined = false;

await step("Start", async () => {
  await waitFor(/All Items/, 60_000);
  await shot("start");
});

await step("Add by DOI", async () => {
  await click(button("Add Item"));
  await type(`//input[@aria-label="DOI, arXiv ID, PMID or ISBN"]`, "10.1038/nature12373");
  await click(`//button[.//span[normalize-space()="Look Up and Add"]]`);
  await waitFor(new RegExp(TITLE), 90_000);
  await shot("doi");
});

await step("Add a scanned PDF", async () => {
  await click(button("Import"));
  await click(menuItem("Add PDF…"));
  answerDialog("Open File", join(files, "scan.pdf"));
  const text = await waitFor(/PDFs added: \d+/, 180_000);
  await shot("scanned-pdf");
  if (/Not recognised/.test(text)) throw new Error("its DOI was not read from the picture");
  joined = /already in the library: 1/.test(text);
  if (!joined) warn("the scanned PDF did not join the paper added by DOI");
});

await step("Open it in the reader", async () => {
  await click(`//button[contains(@class, "link") and normalize-space()=${quote(TITLE)}]`);
  await click(button("Read"), 20_000);
  await find(`//button[@aria-label="Edit Pages" and not(@disabled)]`, 60_000);
  await sleep(3000);
  await shot("reader");
});

await step("Recognize Text", async () => {
  await click(button("Edit Pages"));
  await click(menuItem("Recognize Text"));
  const text = await waitFor(/Pages with recognized text: \d+|Text recognition is not available|No scanned pages/, 120_000);
  await shot("recognized");
  if (!/Pages with recognized text: 1\./.test(text)) throw new Error(text.match(/.*(not available|No scanned pages).*/)?.[0] ?? "no result");
});

await step("Search Everything finds a word only the picture holds", async () => {
  await click(nav("Search Everything"));
  await type(`//input[@placeholder="Words in titles, PDFs, annotations and notes"]`, `lighthouse${ENTER}`);
  const text = await waitFor(/lighthouse keeper|No results|Nothing found/i, 30_000);
  await shot("search");
  if (!/lighthouse keeper/i.test(text)) throw new Error("no hit for a word in the scanned page");
});

const NOTE = "Written by the desktop end-to-end test.";
await step("Write a note", async () => {
  await click(nav("Notes"));
  await click(button("New Note"));
  const editor = await find(`//div[contains(@class, "cm-content")]`, 20_000);
  await wd("POST", at(`/element/${editor}/click`), {});
  await wd("POST", at(`/element/${editor}/value`), { text: NOTE });
  await sleep(3000);
  await shot("note");
  // Leaving the note saves it.
  await click(nav("Library"));
});

await step("Cite in Word Document", async () => {
  await click(button("Export…"));
  await click(menuItem("Cite in Word Document…"));
  await click(button("Choose Document…"));
  answerDialog("Open File", join(files, "paper.docx"));
  answerDialog("Save File", "");
  const text = await waitFor(/Citations formatted: \d+; references: \d+|could not be read|is not a Word document/, 60_000);
  await shot("word");
  if (!/Citations formatted: 1; references: 1\./.test(text)) throw new Error(text.match(/Citations formatted.*|.*could not.*/)?.[0] ?? "no result");
  const cited = join(files, "paper (cited).docx");
  const xml = execFileSync(WINDOWS ? "python" : "python3", ["-c", "import sys, zipfile; print(zipfile.ZipFile(sys.argv[1]).read('word/document.xml').decode())", cited], { encoding: "utf8" });
  const words = xml.replace(/<[^>]+>/g, "");
  if (!words.includes("[1]") || !words.includes(TITLE)) throw new Error(`the copy reads: ${words.slice(0, 300)}`);
  await click(`//div[@role="dialog"]//button[normalize-space()="Done"]`);
});

await step("Open a reference file with Paperloom", async () => {
  const file = join(files, "curie.ris");
  if (WINDOWS) {
    // As a double click in Explorer does: the shell finds the program the
    // installer registered for .ris, which hands the file to the running app.
    execFileSync("cmd.exe", ["/d", "/s", "/c", `start "" "${file}"`], {
      timeout: 30_000, stdio: "ignore", windowsVerbatimArguments: true,
    });
  } else {
    // As a file manager's Open With does while the app runs: the program
    // again, with the file; it hands the file to the running app and quits.
    execFileSync(app, [file], { env: { ...process.env, HOME: APP_HOME }, timeout: 30_000, stdio: "ignore" });
  }
  const text = await waitFor(/Opened in Paperloom[\s\S]*curie\.ris: Added 1,/, 60_000);
  await shot("opened-file");
  if (!/Opened in Paperloom/.test(text)) throw new Error("no dialog for the opened file");
  // The dialog in front, should another still be open behind it.
  await click(`(//div[@role="dialog"])[last()]//button[normalize-space()="Done"]`);
  await click(nav("Library"));
  await waitFor(/Radioactivity/, 15_000);
  await shot("opened-file-added");
});

await step("The note is a file", async () => {
  if (!APP_HOME) throw new Error("APP_HOME is not set");
  const notes = filesUnder(APP_HOME).filter((path) => path.endsWith(".md"));
  const written = notes.find((path) => readFileSync(path, "utf8").includes(NOTE));
  if (!written) throw new Error(`no note holds the text (notes: ${notes.join(", ") || "none"})`);
  console.log(`    ${written}`);
});

await step("Install the Word add-in", async () => {
  // Word's developer add-ins: the manifest's id, the path to the copy.
  const listed = () => {
    try {
      return execFileSync("reg", ["query", "HKCU\\Software\\Microsoft\\Office\\16.0\\WEF\\Developer"], { encoding: "utf8" });
    } catch {
      return "";
    }
  };
  await click(nav("Settings"));
  const row = `//section[h3[normalize-space()="Writing Plugins"]]//div[@class="setting"][span[normalize-space()="Word add-in"]]`;
  await click(`${row}//button[normalize-space()="Install" or normalize-space()="Reinstall" or normalize-space()="Update"]`);
  await waitFor(/Word add-in[\s\S]*Version \S+ installed[\s\S]*Restart Word/);
  await shot("word-add-in");
  const entry = listed().match(/^\s*(\S+)\s+REG_SZ\s+(.+?)\s*$/m);
  if (!entry) throw new Error(`Word lists no developer add-in:\n${listed()}`);
  console.log(`    ${entry[1]} = ${entry[2]}`);
  const manifest = readFileSync(entry[2], "utf8");
  if (!manifest.includes(`<Id>${entry[1]}</Id>`) || !manifest.includes("https://word-addin.paperloom.test/addin.html")) {
    throw new Error(`the listed manifest is not the add-in's: ${manifest.slice(0, 300)}`);
  }
  await click(`${row}//button[normalize-space()="Remove"]`);
  await waitFor(/Not installed · Restart Word/);
  if (listed().includes(entry[1])) throw new Error("the add-in is still listed after Remove");
});

await step("Install the WPS add-in", async () => {
  await click(nav("Settings"));
  const card = `//section[h3[normalize-space()="Writing Plugins"]]`;
  await click(`${card}//button[normalize-space()="Install" or normalize-space()="Reinstall" or normalize-space()="Update"]`);
  await waitFor(/Version \S+ installed/);
  await shot("wps-add-in");
});

await step("Cite in WPS", async () => {
  const document = process.env.WPS_DOCUMENT;
  const reports = process.env.WPS_REPORTS;
  const addons = join(APP_HOME, ".local/share/Kingsoft/wps/jsaddons");
  const folder = existsSync(addons) && readdirSync(addons).find((name) => name.startsWith("paperloom_"));
  if (!folder) throw new Error(`Settings put no add-in in ${addons}`);
  // Test only: the pane opens when WPS loads the add-in, and pane-e2e.js walks it.
  const addin = join(addons, folder);
  appendFileSync(join(addin, "main.js"), `\nsetTimeout(function () { OnAction({ Id: "paperloomCite" }); }, 3000);\n`);
  copyFileSync(join(HERE, "wps-probe/pane-e2e.js"), join(addin, "pane-e2e.js"));
  const pane = join(addin, "pane.html");
  writeFileSync(pane, readFileSync(pane, "utf8").replace("</body>", `<script src="pane-e2e.js"></script>\n</body>`));

  const log = openSync(join(out, "wps.log"), "a");
  spawn("wps", [document], { env: { ...process.env, HOME: APP_HOME }, detached: true, stdio: ["ignore", log, log] }).unref();
  // WPS's first start in a 1440x900 screen: "WPS Office is not your default
  // office software" (OK), may the add-in run (Allow), trust it (OK).
  await sleep(20_000);
  screen("wps-started");
  for (const [x, y] of [[937, 510], [748, 596], [776, 481]]) {
    execFileSync("xdotool", ["mousemove", String(x), String(y), "click", "1"]);
    await sleep(3000);
    screen(`wps-answered-${x}`);
  }

  // The pane asks to connect, and Paperloom asks the person.
  await waitFor(/Allow WPS to use your library\?/, 120_000);
  await shot("wps-asks");
  await click(`//div[@role="alertdialog"]//button[normalize-space()="Allow"]`);

  const until = Date.now() + 180_000;
  let steps = [];
  while (Date.now() < until) {
    steps = existsSync(reports)
      ? readFileSync(reports, "utf8").split("\n").filter(Boolean).map((line) => JSON.parse(line)).filter((r) => r.source === "pane")
      : [];
    if (steps.some((r) => r.step === "done" || r.ok === "0")) break;
    await sleep(2000);
  }
  // Paperloom came to the front to ask; WPS goes back on top for the picture.
  try {
    execFileSync("xdotool", ["search", "--onlyvisible", "--name", "WPS", "windowraise", "%@"]);
    await sleep(1000);
  } catch (e) {
    warn(`raise WPS: ${e.message}`);
  }
  screen("wps-cited");
  console.log(steps.map((r) => `    pane ${r.step}: ${r.detail}`).join("\n"));
  const failedStep = steps.find((r) => r.ok === "0");
  if (failedStep) throw new Error(`the task pane: ${failedStep.detail}`);
  if (!steps.some((r) => r.step === "done")) throw new Error(`the task pane stopped after: ${steps.map((r) => r.step).join(", ") || "nothing"}`);

  const part = (name) => execFileSync("unzip", ["-p", document, name]).toString();
  const body = part("word/document.xml");
  const text = body.replace(/<[^>]+>/g, "");
  const citation = body.split('w:tag w:val="paperloom:cite:')[1] ?? "";
  // Paragraphs' text: the reference list stands apart from the citations and
  // the text they were inserted into.
  const paragraphs = body.split("</w:p>").map((p) => p.replace(/<[^>]+>/g, ""));
  const listed = paragraphs.filter((p) => p.includes(TITLE) || p.includes("Radioactivity"));
  const checks = [
    ["a citation field", /w:tag w:val="paperloom:cite:/.test(body)],
    ["the [@DOI] marker made a citation", !text.includes("[@10.1038") && body.split('w:tag w:val="paperloom:cite:').length === 4],
    ["both papers in the reference list", text.includes(TITLE) && text.includes("Radioactivity")],
    [
      "the reference list in paragraphs of its own",
      listed.length > 0 && listed.every((p) => !p.includes("Temperature can be measured") && !p.includes("[1][2]")),
    ],
    ["the reference list outside the citation", !citation.split("</w:sdt>")[0].includes("<w:sdt>")],
    ["the citation's number", text.includes("[1]")],
    ["a superscript number (GB/T 7714 numeric)", /w:vertAlign w:val="superscript"/.test(body)],
    ["the reference list", /w:tag w:val="paperloom:bibliography"/.test(body)],
    ["Paperloom's data", /w:docVar w:name="paperloom"/.test(part("word/settings.xml"))],
  ];
  const missing = checks.filter(([, ok]) => !ok).map(([what]) => what);
  if (missing.length) throw new Error(`the saved document lacks ${missing.join(", ")}`);
});

await wd("DELETE", at("")).catch(() => undefined);
const summary = [...failed.map((f) => `failed: ${f}`), ...warned.map((w) => `warning: ${w}`)];
writeFileSync(join(out, "summary.txt"), `${summary.join("\n") || "all steps passed"}\n`);
console.log(failed.length ? `Failed: ${failed.length} steps` : "All desktop steps passed.");
process.exit(failed.length ? 1 : 0);
