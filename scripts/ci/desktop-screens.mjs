// Opens the built web UI (app/dist, its in-memory library filled with
// samples by `?demo`) in Chromium (as Windows' WebView2 draws it) and WebKit
// (as Linux's WebKitGTK and Safari do) at several window sizes, walks the
// main desktop screens, and keeps a screenshot of each with a report of
// controls that overlap, run past the window or are cut off. A second pass
// at two sizes shows the app in Simplified Chinese and dark mode. With
// AXE_SCRIPT (axe-core's axe.min.js), every screen in Chromium at 1280 wide
// and at phone width is also audited for accessibility (accessibility.md).
//
//   PLAYWRIGHT_MODULE=…/playwright/index.mjs AXE_SCRIPT=…/axe-core/axe.min.js \
//     node scripts/ci/desktop-screens.mjs app/dist test-results/desktop
//
// Overlaps, cut-off text and accessibility findings are warnings for a
// person to look at; the run fails only when the app does not load or the
// page throws.
import { createReadStream, existsSync, mkdirSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { dirname, extname, join, normalize } from "node:path";

const [dist, out] = process.argv.slice(2);
if (!dist || !out) throw new Error("usage: desktop-screens.mjs <dist> <out>");
const playwright = await import(process.env.PLAYWRIGHT_MODULE ?? "playwright");
const axeSource = process.env.AXE_SCRIPT ? readFileSync(process.env.AXE_SCRIPT, "utf8") : null;
// One engine and the sizes that differ in layout are enough for the audit.
const audited = (engine, width) => axeSource !== null && engine === "chromium" && [1280, 390].includes(width);

const SIZES = [
  [1440, 900],
  [1280, 800],
  [1024, 768],
  [800, 700],
];
const ENGINES = ["chromium", "webkit"];
// The app follows the browser's language and appearance. Labels are found
// through the app's own translations (app/src/i18n/locales).
const VARIANTS = [
  { suffix: "", locale: "en-US", colorScheme: "light", language: "en", sizes: SIZES },
  // 390 wide is a phone: the Android app's layout, with a bottom bar.
  { suffix: "-zh-dark", locale: "zh-CN", colorScheme: "dark", language: "zh-Hans", sizes: [[1280, 800], [800, 700], [390, 844]] },
];
const locales = join(dirname(dist), "src/i18n/locales");
const messages = (language) =>
  language === "en" ? {} : JSON.parse(readFileSync(join(locales, `${language}.json`), "utf8"));
const TYPES = {
  ".html": "text/html",
  ".js": "text/javascript",
  ".mjs": "text/javascript",
  ".css": "text/css",
  ".json": "application/json",
  ".svg": "image/svg+xml",
  ".png": "image/png",
  ".woff2": "font/woff2",
  ".wasm": "application/wasm",
  ".map": "application/json",
};

// A static server for dist/, falling back to index.html.
const server = createServer((request, response) => {
  const path = normalize(decodeURIComponent((request.url ?? "/").split("?")[0])).replace(/^(\.\.[/\\])+/, "");
  let file = join(dist, path);
  if (!existsSync(file) || statSync(file).isDirectory()) file = join(dist, "index.html");
  response.setHeader("Content-Type", TYPES[extname(file)] ?? "application/octet-stream");
  createReadStream(file).pipe(response);
});
await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
const base = `http://127.0.0.1:${server.address().port}`;

// Runs in the page: what overlaps, runs past the window or is cut off.
function inspect() {
  const vw = innerWidth;
  const vh = innerHeight;
  // The part of an element one can see: inside the window and inside every
  // ancestor that clips (scroll areas, hidden overflow).
  const seen = (el) => {
    const r = el.getBoundingClientRect();
    let [left, top, right, bottom] = [Math.max(r.left, 0), Math.max(r.top, 0), Math.min(r.right, vw), Math.min(r.bottom, vh)];
    for (let e = el.parentElement; e && e !== document.documentElement; e = e.parentElement) {
      const o = getComputedStyle(e);
      if (o.overflowX === "visible" && o.overflowY === "visible") continue;
      const p = e.getBoundingClientRect();
      left = Math.max(left, p.left);
      top = Math.max(top, p.top);
      right = Math.min(right, p.right);
      bottom = Math.min(bottom, p.bottom);
    }
    return { left, top, right, bottom, width: right - left, height: bottom - top };
  };
  const shown = (el) => {
    const s = getComputedStyle(el);
    const v = seen(el);
    return v.width > 1 && v.height > 1 && s.visibility !== "hidden" && s.display !== "none" && Number(s.opacity) > 0.05;
  };
  // Inside a row that scrolls sideways, and not wholly in sight.
  const offRow = (el) => {
    const r = el.getBoundingClientRect();
    for (let e = el.parentElement; e && e !== document.body; e = e.parentElement) {
      const o = getComputedStyle(e).overflowX;
      if ((o === "auto" || o === "scroll") && e.scrollWidth > e.clientWidth + 1) {
        const p = e.getBoundingClientRect();
        return r.left < p.left - 1 || r.right > p.right + 1 ? e : null;
      }
    }
    return null;
  };
  const name = (el) =>
    (el.getAttribute("aria-label") || el.getAttribute("placeholder") || el.textContent || el.value || "")
      .trim()
      .replace(/\s+/g, " ")
      .slice(0, 60);
  const where = (el) => {
    const parts = [];
    for (let e = el; e && e !== document.body && parts.length < 4; e = e.parentElement) {
      const cls = [...e.classList].slice(0, 2).join(".");
      parts.unshift(e.tagName.toLowerCase() + (cls ? `.${cls}` : ""));
    }
    return parts.join(" > ");
  };
  const box = (r) => [Math.round(r.left), Math.round(r.top), Math.round(r.width), Math.round(r.height)];
  // Something that scrolls sideways on purpose (a row of chips, a table).
  const scrolls = (el) => {
    for (let e = el.parentElement; e && e !== document.body; e = e.parentElement) {
      const o = getComputedStyle(e).overflowX;
      if ((o === "auto" || o === "scroll") && e.scrollWidth > e.clientWidth + 1) return true;
    }
    return false;
  };
  // With a dialog open, only the dialog counts: it covers the page on purpose.
  const dialog = [...document.querySelectorAll(".dialog-backdrop, [role=dialog]")].filter(shown).pop();
  const scope = dialog ?? document;
  const controls = [
    ...scope.querySelectorAll("button, a, input, select, textarea, label, h1, h2, h3, h4, th, [role=button], [role=tab], .chip"),
  ].filter(shown);

  const pastWindow = [];
  const cut = [];
  const hidden = [];
  const sideways = [];
  for (const el of [...scope.querySelectorAll("button, a, input, select, [role=tab]")]) {
    const s = getComputedStyle(el);
    if (s.display === "none" || s.visibility === "hidden") continue;
    const row = offRow(el);
    if (row) sideways.push({ what: name(el), where: where(el), row: where(row) });
  }
  for (const el of controls) {
    const r = el.getBoundingClientRect();
    if ((r.right > vw + 1 || r.left < -1) && !scrolls(el)) pastWindow.push({ what: name(el), where: where(el), box: box(r) });
    const s = getComputedStyle(el);
    if (el.scrollWidth > el.clientWidth + 1 && s.overflowX !== "visible" && !["INPUT", "TEXTAREA", "SELECT"].includes(el.tagName)) {
      cut.push({ what: name(el), where: where(el), box: box(r), ellipsis: s.textOverflow === "ellipsis" });
    }
    // Part of it hidden by a parent that clips and does not scroll.
    for (let e = el.parentElement; e && e !== document.body; e = e.parentElement) {
      const o = getComputedStyle(e);
      if (o.overflowX === "visible" && o.overflowY === "visible") continue;
      if (o.overflowX === "auto" || o.overflowX === "scroll" || o.overflowY === "auto" || o.overflowY === "scroll") break;
      const p = e.getBoundingClientRect();
      if (r.right > p.right + 2 || r.left < p.left - 2) hidden.push({ what: name(el), where: where(el), box: box(r), by: where(e) });
      break;
    }
  }

  const overlaps = [];
  for (let i = 0; i < controls.length; i++) {
    for (let j = i + 1; j < controls.length; j++) {
      const a = controls[i];
      const b = controls[j];
      if (a.contains(b) || b.contains(a)) continue;
      if (a.closest("button") && a.closest("button") === b.closest("button")) continue;
      // A label and the field it names sit together on purpose.
      if ((a.tagName === "LABEL" && a.control === b) || (b.tagName === "LABEL" && b.control === a)) continue;
      const ra = seen(a);
      const rb = seen(b);
      const w = Math.min(ra.right, rb.right) - Math.max(ra.left, rb.left);
      const h = Math.min(ra.bottom, rb.bottom) - Math.max(ra.top, rb.top);
      if (w > 4 && h > 4) {
        // Which one is on top where they meet.
        const x = Math.max(ra.left, rb.left) + w / 2;
        const y = Math.max(ra.top, rb.top) + h / 2;
        const top = document.elementFromPoint(x, y);
        overlaps.push({
          a: name(a), aWhere: where(a), aBox: box(ra),
          b: name(b), bWhere: where(b), bBox: box(rb),
          onTop: top && (a.contains(top) ? "a" : b.contains(top) ? "b" : "other"),
        });
      }
    }
  }
  return { pastWindow, cut, hidden, sideways: sideways.slice(0, 40), overlaps: overlaps.slice(0, 40), controls: controls.length };
}

// Runs in the page once axe-core is loaded: what breaks WCAG 2.2 AA and
// axe's best practices, with a few of the elements each rule names.
async function audit() {
  // Frames (the built-in browser, plugin sandboxes) have no axe of their
  // own; asking them would wait out axe's frame timeout on every screen.
  const result = await window.axe.run(document, { resultTypes: ["violations"], iframes: false });
  return result.violations.map((v) => ({
    rule: v.id,
    impact: v.impact,
    help: v.help,
    count: v.nodes.length,
    nodes: v.nodes.slice(0, 6).map((n) => ({
      target: n.target.join(" "),
      html: n.html.replace(/\s+/g, " ").slice(0, 160),
      why: (n.failureSummary ?? "").split("\n").slice(1, 2).join(" ").trim(),
    })),
  }));
}

rmSync(out, { recursive: true, force: true });
const report = [];
let broken = false;

for (const engine of ENGINES) {
  const browser = await playwright[engine].launch();
  for (const variant of VARIANTS) {
    const translated = messages(variant.language);
    const l = (key) => translated[key] ?? key;
    for (const [width, height] of variant.sizes) {
      const dir = join(out, `${engine}-${width}${variant.suffix}`);
      mkdirSync(dir, { recursive: true });
      const context = await browser.newContext({
        viewport: { width, height },
        locale: variant.locale,
        colorScheme: variant.colorScheme,
      });
      const page = await context.newPage();
      const errors = [];
      page.on("pageerror", (e) => errors.push(`page error: ${e.message}`));
      page.on("console", (m) => m.type() === "error" && errors.push(`console: ${m.text()}`));
      let n = 0;
      const shot = async (step, notes = []) => {
        n += 1;
        const file = `${String(n).padStart(2, "0")}-${step}.png`;
        await page.waitForTimeout(400);
        await page.screenshot({ path: join(dir, file) });
        const found = await page.evaluate(inspect);
        let accessibility;
        if (audited(engine, width)) {
          try {
            if (!(await page.evaluate(() => "axe" in window))) await page.evaluate(axeSource);
            accessibility = await page.evaluate(audit);
          } catch (e) {
            notes = [...notes, `accessibility audit failed: ${String(e.message ?? e).split("\n")[0]}`];
          }
        }
        report.push({ engine: engine + variant.suffix, width, height, step, file: `${engine}-${width}${variant.suffix}/${file}`, notes, accessibility, ...found });
      };
      // Steps whose control is missing are noted, not fatal.
      const step = async (name, act) => {
        try {
          await act();
          await shot(name);
        } catch (e) {
          await shot(`${name}-failed`, [String(e.message ?? e).split("\n")[0]]).catch(() => undefined);
        }
      };
      // The activity bar, or on a phone the bottom bar and its More menu.
      const activity = async (label) => {
        if (await page.locator("nav.activity-bar").isVisible()) {
          await page.locator(`nav.activity-bar button[aria-label="${l(label)}"]`).click({ timeout: 5000 });
          return;
        }
        const bottom = page.locator(`nav.bottom-nav button[aria-label="${l(label)}"]`);
        if (await bottom.count()) {
          await bottom.first().click({ timeout: 5000 });
          return;
        }
        await page.locator(`nav.bottom-nav button[aria-label="${l("More")}"]`).click({ timeout: 5000 });
        // Items that show which tab is open are checkable menu items.
        await page.locator('[role="menu"] [role^="menuitem"]', { hasText: l(label) }).first().click({ timeout: 5000 });
      };

      try {
        await page.goto(`${base}/?demo`);
        await page.getByText("A Mathematical Theory of Communication").first().waitFor({ timeout: 20000 });
      } catch (e) {
        broken = true;
        report.push({ engine: engine + variant.suffix, width, height, step: "load", notes: [`the app did not load: ${e.message.split("\n")[0]}`] });
        await page.screenshot({ path: join(dir, "00-not-loaded.png") }).catch(() => undefined);
        await context.close();
        continue;
      }
      await shot("library");
      await step("item", async () => {
        await page.locator("table.items button.link").first().click({ timeout: 5000 });
        await page.getByText(l("Cite")).first().waitFor({ timeout: 5000 });
      });
      await step("split-view", async () => {
        await page.locator(`button[aria-label="${l("Open in Split View")}"]`).first().click({ timeout: 5000 });
        await page.locator(".editor.split").waitFor({ timeout: 5000 });
      });
      const closeSplit = page.locator(`button[aria-label="${l("Close Split View")}"]`).first();
      if (await closeSplit.isVisible().catch(() => false)) await closeSplit.click();
      await step("search-everything", () => activity("Search Everything"));
      await step("online-search", () => activity("Online Search"));
      await step("browser", () => activity("Browser"));
      await step("notes", () => activity("Notes"));
      await step("new-note", async () => {
        await page.locator(`.vault-tree button[aria-label="${l("New")}"]`).first().click({ timeout: 5000 });
        await page.getByRole("menuitem", { name: l("New Note") }).first().click({ timeout: 5000 });
      });
      await step("settings", () => activity("Settings"));
      await step("settings-scrolled", async () => {
        await page.locator(".editor-body").first().evaluate((e) => e.scrollTo(0, e.scrollHeight));
      });
      await step("add-item", async () => {
        await activity("Library");
        await page.locator(`button[aria-label="${l("Add Item")}"]`).first().click({ timeout: 5000 });
        await page.getByRole("dialog").first().waitFor({ timeout: 5000 });
      });
      await page.keyboard.press("Escape");

      if (errors.some((e) => e.startsWith("page error"))) broken = true;
      if (errors.length) report.push({ engine: engine + variant.suffix, width, height, step: "errors", notes: errors.slice(0, 20) });
      await context.close();
    }
  }
  await browser.close();
}
server.close();

// The report: counts per screen, then the details worth a look.
const lines = [
  "# Desktop layout",
  "",
  "| Screen | Controls | Past window | Cut off (no …) | Hidden by parent | Scroll a row to reach | Overlaps |",
  "| --- | --- | --- | --- | --- | --- | --- |",
];
for (const r of report) {
  if (!r.file) continue;
  const cut = (r.cut ?? []).filter((c) => !c.ellipsis).length;
  lines.push(`| ${r.file} | ${r.controls} | ${r.pastWindow.length} | ${cut} | ${r.hidden.length} | ${r.sideways.length} | ${r.overlaps.length} |`);
}
lines.push("");
for (const r of report) {
  const items = [
    ...(r.notes ?? []).map((n) => `- note: ${n}`),
    ...(r.pastWindow ?? []).map((p) => `- past the window: "${p.what}" ${p.where} ${p.box}`),
    ...(r.cut ?? []).filter((c) => !c.ellipsis).map((c) => `- cut off: "${c.what}" ${c.where} ${c.box}`),
    ...(r.hidden ?? []).map((h) => `- hidden by ${h.by}: "${h.what}" ${h.box}`),
    ...(r.sideways ?? []).map((w) => `- scroll ${w.row} to reach: "${w.what}"`),
    ...(r.overlaps ?? []).map((o) => `- overlap: "${o.a}" (${o.aWhere} ${o.aBox}) / "${o.b}" (${o.bWhere} ${o.bBox}), on top: ${o.onTop}`),
  ];
  if (items.length) lines.push(`## ${r.engine} ${r.width}×${r.height} — ${r.step}`, "", ...items, "");
}
writeFileSync(join(out, "report.md"), lines.join("\n"));

// The accessibility audit: each rule once, with every screen it was found
// on and the elements it named there.
if (axeSource) {
  const rules = new Map();
  for (const r of report) {
    for (const v of r.accessibility ?? []) {
      const rule = rules.get(v.rule) ?? { ...v, screens: [], nodes: new Map() };
      rule.screens.push(`${r.file} (${v.count})`);
      for (const n of v.nodes) if (!rule.nodes.has(n.target)) rule.nodes.set(n.target, n);
      rules.set(v.rule, rule);
    }
  }
  const order = { critical: 0, serious: 1, moderate: 2, minor: 3 };
  const sorted = [...rules.values()].sort((a, b) => (order[a.impact] ?? 4) - (order[b.impact] ?? 4) || a.rule.localeCompare(b.rule));
  const screens = report.filter((r) => r.accessibility).length;
  const text = [
    "# Accessibility (axe-core)",
    "",
    `${screens} screens audited in Chromium at 1280 and 390 wide; ${sorted.length} rules broken.`,
    "",
    "| Rule | Impact | Screens | What it asks |",
    "| --- | --- | --- | --- |",
    ...sorted.map((v) => `| ${v.rule} | ${v.impact} | ${v.screens.length} | ${v.help} |`),
    "",
  ];
  for (const v of sorted) {
    text.push(`## ${v.rule} (${v.impact}): ${v.help}`, "", `Screens: ${v.screens.join(", ")}`, "");
    for (const n of [...v.nodes.values()].slice(0, 12)) text.push(`- \`${n.target}\` ${n.html}${n.why ? ` — ${n.why}` : ""}`);
    text.push("");
  }
  writeFileSync(join(out, "accessibility.md"), text.join("\n"));
  console.log(text.slice(0, 6 + sorted.length).join("\n"));
}
writeFileSync(join(out, "report.json"), JSON.stringify(report, null, 1));
console.log(lines.slice(0, 4 + report.length).join("\n"));
if (broken) {
  console.error("The app did not load or the page threw; see report.md.");
  process.exit(1);
}
