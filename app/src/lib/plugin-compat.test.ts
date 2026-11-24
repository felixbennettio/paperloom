// Real Zotero plugins started in the shared runtime, each in its own
// window, as the apps do. The plugins are not part of the repository: the
// Nightly unpacks pinned releases into a folder and points
// PAPERLOOM_PLUGIN_COMPAT at it; without it these tests are skipped.
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";
import { Window } from "happy-dom";
import { describe, expect, it, vi } from "vitest";
import { EditorSelection, EditorState } from "@codemirror/state";
import { history, undo, undoDepth } from "@codemirror/commands";
import { pluginTransaction } from "../components/NoteEditor";
import { validatePluginDecorations } from "./pluginEditor";
import type { EditorProposal, EditorSnapshot } from "./plugins";

const root = process.env.PAPERLOOM_PLUGIN_COMPAT;
const runtime = readFileSync(new URL("../../../core/plugin-runtime/zotero.js", import.meta.url), "utf8");
const obsidianRuntime = readFileSync(new URL("../../../core/plugin-runtime/obsidian.js", import.meta.url), "utf8");
const TEXT = /\.(js|mjs|json|ftl|xhtml|html|css|txt|svg|properties|dtd)$/i;

/** What each plugin must show once started: a column, item menu entries. */
const EXPECTED: Record<string, { column?: string; menu?: string }> = {
  "reading-list": { column: "Read Status", menu: "Read Status" },
  "zotero-actions-tags": { menu: "Trigger Action" },
  "zotero-format-metadata": { menu: "Lint" },
  "pdf-translate": {},
};

function textFiles(dir: string) {
  const files: { path: string; text: string }[] = [];
  const walk = (folder: string) => {
    for (const name of readdirSync(folder)) {
      const path = join(folder, name);
      if (statSync(path).isDirectory()) walk(path);
      else if (TEXT.test(name)) files.push({ path: relative(dir, path).replaceAll("\\", "/"), text: readFileSync(path, "utf8") });
    }
  };
  walk(dir);
  return files;
}

async function startPlugin(dir: string) {
  const manifest = JSON.parse(readFileSync(join(dir, "manifest.json"), "utf8"));
  const id = manifest.applications?.zotero?.id ?? manifest.browser_specific_settings?.zotero?.id;
  const window = new Window({ url: "http://plugin.localhost/zotero.html" });
  const messages: Record<string, unknown>[] = [];
  const app = {
    postMessage(message: Record<string, unknown>) {
      messages.push(message);
      // The app's answers to the runtime's calls: storage succeeds, the web is offline.
      if (message.type === "call") {
        const ok = message.method !== "requestUrl";
        queueMicrotask(() => send({ type: "reply", id: message.id, ok, value: null, error: ok ? undefined : "offline" }));
      }
    },
  };
  Object.defineProperty(window, "parent", { value: app });
  const send = (data: object) => window.dispatchEvent(new window.MessageEvent("message", { data, source: app as never }));
  window.eval(runtime);
  const items = [
    { key: "AAAA1111", itemType: "journalArticle", title: "A CASE STUDY", creators: [{ firstName: "ada", lastName: "lovelace", creatorType: "author" }],
      date: "2024-02-29", DOI: "10.1000/xyz", extra: "", tags: [], attachments: [{ key: "PDF11111", contentType: "application/pdf" }] },
  ];
  send({ type: "start", manifest: { id, name: manifest.name, version: manifest.version },
    code: { files: textFiles(dir), prefs: null }, items, seq: 1, selection: { seq: 1, itemKeys: [] } });
  const settled = (type: string) => messages.some((message) => message.type === type);
  for (let waited = 0; waited < 15_000 && !settled("ready") && !settled("error"); waited += 50) {
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  // Startup hooks that are not awaited still run after "ready".
  await new Promise((resolve) => setTimeout(resolve, 1500));
  return { window, messages, send };
}

async function itemMenu(send: (data: object) => void, messages: Record<string, unknown>[]) {
  send({ type: "item-menu", id: 900, itemKeys: ["AAAA1111"] });
  await new Promise((resolve) => setTimeout(resolve, 300));
  const reply = messages.find((message) => message.type === "item-menu" && message.id === 900);
  const labels: string[] = [];
  const collect = (entries: { label?: string; children?: unknown[] }[]) => {
    for (const entry of entries ?? []) {
      if (entry.label) labels.push(entry.label);
      if (entry.children) collect(entry.children as typeof entries);
    }
  };
  collect((reply?.items as never) ?? []);
  return labels;
}

const isObsidian = (dir: string) => "minAppVersion" in JSON.parse(readFileSync(join(dir, "manifest.json"), "utf8"));
const pluginDirs = root ? readdirSync(root).filter((entry) => statSync(join(root, entry)).isDirectory()) : [];

/** A vault of two notes, as the app describes it to Obsidian plugins. */
const NOTES: Record<string, string> = {
  "Notes/Alpha.md": "---\nstatus: done\n---\n# Alpha\n",
  "Notes/Beta.md": "---\nstatus: open\n---\n# Beta\n",
};

/** happy-dom has no Worker; Dataview parses notes in one made from a Blob.
 * The worker's script runs in the page's own realm (so instanceof checks
 * on received Maps hold, as with a browser's structured clone) inside its
 * own scope, and messages pass between them asynchronously. */
function installWorkers(window: Window) {
  type Text = { text(): Promise<string> };
  const blobs = new Map<string, Text>();
  const urls = window.URL as unknown as { createObjectURL(blob: Text): string };
  const created = urls.createObjectURL.bind(urls);
  urls.createObjectURL = (blob) => {
    const url = created(blob);
    blobs.set(url, blob);
    return url;
  };
  type Listener = (event: { data: unknown }) => void;
  class Worker {
    onmessage: Listener | null = null;
    private scope: Record<string, unknown> & { onmessage: Listener | null };
    private ready: Promise<void>;
    constructor(url: string) {
      const scope = {
        onmessage: null as Listener | null,
        postMessage: (data: unknown) => setTimeout(() => this.onmessage?.({ data }), 0),
        addEventListener: (type: string, listener: Listener) => { if (type === "message") scope.onmessage = listener; },
        removeEventListener: () => undefined,
        close: () => undefined,
      } as Record<string, unknown> & { onmessage: Listener | null };
      scope.self = scope;
      this.scope = scope;
      const run = (window as unknown as { Function: FunctionConstructor }).Function;
      this.ready = blobs.get(String(url))!.text().then((code) => { run("scope", `with (scope) {\n${code}\n}`)(scope); });
    }
    postMessage(data: unknown) {
      void this.ready.then(() => setTimeout(() => this.scope.onmessage?.({ data }), 0));
    }
    addEventListener(type: string, listener: Listener) {
      if (type === "message") this.onmessage = listener;
    }
    removeEventListener() {}
    terminate() {}
  }
  Object.assign(window, { Worker });
}

async function startObsidian(dir: string) {
  const manifest = JSON.parse(readFileSync(join(dir, "manifest.json"), "utf8"));
  const window = new Window({ url: "http://plugin.localhost/host.html" });
  const messages: Record<string, unknown>[] = [];
  const app = {
    postMessage(message: Record<string, unknown>) {
      messages.push(message);
      if (message.type === "call") {
        const [first] = (message.args as unknown[]) ?? [];
        // The app reads notes and renders Markdown; links become their text here.
        const markdown = (text: string) => `<p>${text.replace(/\[\[([^\]|]+)(\|[^\]]*)?\]\]/g, "$1").replace(/</g, "&lt;")}</p>`;
        const value = message.method === "vault.read" ? NOTES[String(first)] ?? null
          : message.method === "markdown.render" ? markdown(String(first ?? "")) : null;
        queueMicrotask(() => send({ type: "reply", id: message.id, ok: true, value }));
      }
    },
  };
  Object.defineProperty(window, "parent", { value: app });
  const send = (data: object) => window.dispatchEvent(new window.MessageEvent("message", { data, source: app as never }));
  installWorkers(window);
  window.eval(readFileSync(new URL("../../../core/plugin-runtime/cm6.js", import.meta.url), "utf8"));
  window.eval(obsidianRuntime);
  const now = Date.now();
  const files = Object.keys(NOTES).map((path) => ({ path, stat: { ctime: now, mtime: now, size: NOTES[path].length } }));
  const metadata = Object.fromEntries(Object.entries(NOTES).map(([path, text]) =>
    [path, { frontmatter: { status: /status: (\w+)/.exec(text)![1] } }]));
  const read = (name: string) => { try { return readFileSync(join(dir, name), "utf8"); } catch { return null; } };
  send({ type: "start", manifest, code: { main: read("main.js"), styles: read("styles.css"), data: null }, files, metadata, platform: {} });
  for (let waited = 0; waited < 15_000 && !messages.some((m) => m.type === "ready" || m.type === "error"); waited += 50) {
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  return { window, messages, send };
}

describe.skipIf(!root || !pluginDirs.some((entry) => isObsidian(join(root!, entry))))("Obsidian plugin compatibility", () => {
  for (const name of pluginDirs.filter((entry) => isObsidian(join(root!, entry)))) {
    it(`${name} starts without errors and supports its compatibility gate`, async () => {
      const { window, messages, send } = await startObsidian(join(root!, name));
      try {
        expect(messages.filter((message) => message.type === "error").map((message) => message.message)).toEqual([]);
        expect(messages.some((message) => message.type === "ready")).toBe(true);
        const languages = messages.filter((message) => message.type === "codeblock").map((message) => message.language);
        if (name === "dataview") {
          expect(languages).toContain("dataview");
          // Dataview indexes the vault in its worker before it can answer.
          await new Promise((resolve) => setTimeout(resolve, 3000));
          send({ type: "render-code", id: 901, language: "dataview", source: 'TABLE status FROM "Notes"', path: "Notes/Alpha.md" });
          for (let waited = 0; waited < 10_000 && !messages.some((m) => m.type === "rendered" && m.id === 901); waited += 50) {
            await new Promise((resolve) => setTimeout(resolve, 50));
          }
          const html = String(messages.find((m) => m.type === "rendered" && m.id === 901)?.html ?? "");
          expect(html).toContain("Alpha");
          expect(html).toContain("done");
          const value = "status:: done\n[owner:: Alice]\n```\n[code:: untouched]\nstatus:: untouched\n```\nend";
          let editor: EditorSnapshot & { path: string } = { protocol: 1, session: "compat-dataview", generation: 1,
            revision: 0, contextRevision: 0, path: "Notes/Alpha.md", value, ranges: [{ anchor: value.length, head: value.length }],
            mainIndex: 0, mode: "live", focused: true, composing: false };
          const latest = () => messages.filter(m => m.type === "decoration/result" && m.contextRevision === editor.contextRevision).at(-1);
          send({ type: "editor/open", editor });
          await vi.waitFor(() => expect(latest()).toBeTruthy());
          const validated = () => validatePluginDecorations("dataview", latest()!, editor)!;
          await vi.waitFor(() => expect(validated().decorations.some(d => d.kind === "replace" && d.text?.includes("Alice"))).toBe(true));
          // Dataview itself prettifies only bracketed fields; a full-line field stays source.
          expect(validated().decorations.every(d => d.from >= value.indexOf("\n"))).toBe(true);
          expect(validated().decorations.every(d => d.to <= value.indexOf("```"))).toBe(true);
          expect(validated().decorations.find(d => d.text?.includes("Alice"))?.spans).toContainEqual(expect.objectContaining({ attributes: expect.objectContaining({ class: "dataview inline-field-key" }) }));
          editor = { ...editor, contextRevision: 1, ranges: [{ anchor: 3, head: 3 }] };
          send({ type: "editor/update", editor });
          await vi.waitFor(() => expect(latest()).toBeTruthy());
          expect(validated().decorations.every(d => d.from >= value.indexOf("\n"))).toBe(true);
          editor = { ...editor, contextRevision: 2, mode: "source" };
          send({ type: "editor/update", editor });
          await vi.waitFor(() => expect(latest()).toBeTruthy());
          expect(validated().decorations).toEqual([]);
          // Use the real setting's callback path (mutable cmExtension array).
          window.eval('DataviewAPI.app.plugins.plugins.dataview.settings.prettyRenderInlineFieldsInLivePreview = false; DataviewAPI.app.plugins.plugins.dataview.updateEditorExtensions()');
          editor = { ...editor, contextRevision: 3, mode: "live", ranges: [{ anchor: value.length, head: value.length }] };
          send({ type: "editor/update", editor });
          await vi.waitFor(() => expect(latest()).toBeTruthy());
          expect(validated().decorations).toEqual([]);
          window.eval('DataviewAPI.app.plugins.plugins.dataview.settings.prettyRenderInlineFieldsInLivePreview = true; DataviewAPI.app.plugins.plugins.dataview.updateEditorExtensions()');
          await vi.waitFor(() => expect(validated().decorations.some(d => d.kind === "replace" && d.text?.includes("Alice"))).toBe(true));
          // The token shim also exposes inline-query content, excluding backticks.
          editor = { ...editor, value: "`= 1 + 2`\nnext", revision: 1, contextRevision: 4,
            ranges: [{ anchor: 14, head: 14 }] };
          send({ type: "editor/update", editor });
          await vi.waitFor(() => expect(validated()?.decorations.some(d => d.kind === "replace" && d.text === "3")).toBe(true));
          // Follow an actual editing sequence: reveal, edit, then leave the line.
          editor = { ...editor, contextRevision: 5, ranges: [{ anchor: 3, head: 3 }] };
          send({ type: "editor/update", editor });
          await vi.waitFor(() => expect(latest()?.decorations).toEqual([]));
          editor = { ...editor, value: "`= 2 + 3`\nnext", revision: 2, contextRevision: 6 };
          send({ type: "editor/update", editor });
          editor = { ...editor, contextRevision: 7, ranges: [{ anchor: 14, head: 14 }] };
          send({ type: "editor/update", editor });
          await vi.waitFor(() => expect(validated()?.decorations.some(d => d.kind === "replace" && d.text === "5")).toBe(true));
        }
        if (name === "nldates" || name === "various-complements") {
          const snapshot = (value: string, revision: number): EditorSnapshot & { path: string } => ({ protocol: 1,
            session: `compat-${name}`, generation: 1, revision, contextRevision: revision, path: "Notes/Alpha.md", value,
            ranges: [{ anchor: value.length, head: value.length }], mainIndex: 0, mode: "source", focused: true, composing: false });
          if (name === "nldates") {
            for (const shiftKey of [false, true]) {
              send({ type: "active", editor: snapshot("", 0) });
              for (const [index, value] of ["@", "@t", "@to", "@tom"].entries()) {
                const editor = snapshot(value, index + 1);
                send({ type: "editor/update", editor });
                send({ type: "suggest/query", request: index + 1, editor });
                await vi.waitFor(() => expect(messages.some(m => m.type === "suggest/result" && m.request === index + 1)).toBe(true));
              }
              const result = messages.filter(m => m.type === "suggest/result" && m.request === 4).at(-1)!;
              const items = result.items as { id: string; label: string }[];
              expect(items).toContainEqual({ id: "4:0", label: "Tomorrow", secondary: undefined });
              const editor = snapshot("@tom", 4);
              const before = messages.filter(m => m.type === "editor/propose").length;
              send({ ...result, type: "suggest/choose", id: items[0].id, key: "Enter", shiftKey });
              await vi.waitFor(() => expect(messages.filter(m => m.type === "editor/propose")).toHaveLength(before + 1));
              const proposal = messages.filter(m => m.type === "editor/propose").at(-1)! as unknown as EditorProposal;
              const tomorrow = new Date(); tomorrow.setDate(tomorrow.getDate() + 1);
              const date = `${tomorrow.getFullYear()}-${String(tomorrow.getMonth() + 1).padStart(2, "0")}-${String(tomorrow.getDate()).padStart(2, "0")}`;
              expect(proposal.edits).toEqual([{ from: 0, to: 4, insert: `[[${date}${shiftKey ? "|Tomorrow" : ""}]]` }]);
              const state = EditorState.create({ doc: "@tom", selection: EditorSelection.cursor(4) });
              expect(pluginTransaction(state, editor, proposal)).not.toBeNull();
              // Start a new session for the alternative key path.
              messages.splice(0, messages.length, ...messages.filter(m => m.type !== "suggest/result"));
            }
          } else {
            // The word exists only in the editor snapshot, not in the saved vault fixture.
            const editor = snapshot("PaperloomCompletion PaperloomCom", 1);
            send({ type: "active", editor });
            await new Promise(resolve => setTimeout(resolve, 200));
            send({ type: "suggest/query", request: 990, editor });
            await vi.waitFor(() => expect(messages.some(m => m.type === "suggest/result" && m.request === 990)).toBe(true), { timeout: 3000 });
            const result = messages.find(m => m.type === "suggest/result" && m.request === 990)!;
            expect(messages.filter(m => m.type === "error").map(m => m.message)).toEqual([]);
            expect((result.items as { label: string }[]).map(item => item.label)).toContain("PaperloomCompletion");
            const item = (result.items as { id: string; label: string }[]).find(item => item.label === "PaperloomCompletion")!;
            send({ ...result, type: "suggest/choose", id: item.id, key: "Enter" });
            await vi.waitFor(() => { expect(messages.filter(m => m.type === "error").map(m => m.message)).toEqual([]); expect(messages.some(m => m.type === "editor/propose")).toBe(true); });
            expect(messages.find(m => m.type === "editor/propose")).toMatchObject({ edits: [expect.objectContaining({ insert: expect.stringContaining("PaperloomCompletion") })] });
            const linkEditor = snapshot("Alp", 2);
            send({ type: "active", editor: linkEditor });
            await new Promise(resolve => setTimeout(resolve, 100));
            send({ type: "suggest/query", request: 991, editor: linkEditor });
            await vi.waitFor(() => expect(messages.some(m => m.type === "suggest/result" && m.request === 991)).toBe(true));
            const links = messages.find(m => m.type === "suggest/result" && m.request === 991)!;
            const link = (links.items as { id: string; label: string }[]).find(item => item.label === "Alpha")!;
            expect(link).toBeTruthy();
            send({ ...links, type: "suggest/choose", id: link.id, key: "Enter" });
            await vi.waitFor(() => { expect(messages.filter(m => m.type === "error").map(m => m.message)).toEqual([]); expect(messages.filter(m => m.type === "editor/propose")).toHaveLength(2); });
            expect(messages.filter(m => m.type === "editor/propose").at(-1)).toMatchObject({ edits: [{ from: 0, to: 3, insert: "[[Alpha]]" }] });
          }
          expect(messages.filter(m => m.type === "error").map(m => m.message)).toEqual([]);
        }
        if (name === "highlightr") {
          const value = "before 😀e\u0301 after";
          const ranges = [{ anchor: 11, head: 7 }];
          let snapshot: EditorSnapshot & { path: string } = { protocol: 1, session: "compat-highlightr", generation: 1,
            revision: 0, contextRevision: 0, path: "Notes/Alpha.md", value, ranges, mainIndex: 0,
            mode: "source", focused: true, composing: false };
          let state = EditorState.create({ doc: value, selection: EditorSelection.range(11, 7), extensions: [history()] });
          const commands = messages.filter(m => m.type === "commands").flatMap(m => m.commands as { id: string }[]);
          expect(commands.some(c => c.id === "highlightr-plugin:Yellow")).toBe(true);
          send({ type: "editor/open", editor: snapshot });
          send({ type: "command/run", id: "highlightr-plugin:Yellow", editor: snapshot });
          await vi.waitFor(() => { expect(messages.filter(m => m.type === "error").map(m => m.message)).toEqual([]); expect(messages.some(m => m.type === "editor/propose")).toBe(true); });
          const proposal = messages.find(m => m.type === "editor/propose") as unknown as EditorProposal;
          const marked = '<mark style="background: #FFF3A3A6;">😀e\u0301</mark>';
          expect(proposal).toMatchObject({ session: snapshot.session, baseRevision: 0, edits: [{ from: 7, to: 11, insert: marked }] });
          state = state.update(pluginTransaction(state, snapshot, proposal)!).state;
          expect(state.doc.toString()).toBe(`before ${marked} after`);
          // Highlightr explicitly sets the cursor one character past the closing tag.
          expect(state.selection.main.head).toBe(7 + marked.length + 1);
          expect(undoDepth(state)).toBe(1);
          let reverted = state;
          expect(undo({ state, dispatch: tr => { reverted = tr.state; } })).toBe(true);
          expect(reverted.doc.toString()).toBe(value);
          expect(reverted.selection.main.anchor).toBe(11);
          expect(reverted.selection.main.head).toBe(7);
          // Exercise the real erase command on exactly the highlighted range.
          snapshot = { ...snapshot, value: state.doc.toString(), revision: 1, contextRevision: 1,
            ranges: [{ anchor: 7 + marked.length, head: 7 }] };
          state = state.update({ selection: EditorSelection.range(7 + marked.length, 7) }).state;
          send({ type: "editor/ack", operation: proposal.operation, editor: snapshot });
          send({ type: "command", id: "highlightr-plugin:unhighlight", editor: snapshot });
          await vi.waitFor(() => expect(messages.filter(m => m.type === "editor/propose")).toHaveLength(2));
          const erase = messages.filter(m => m.type === "editor/propose")[1] as unknown as EditorProposal;
          expect(erase.edits).toEqual([{ from: 7, to: 7 + marked.length, insert: "😀e\u0301" }]);
          state = state.update(pluginTransaction(state, snapshot, erase)!).state;
          expect(state.doc.toString()).toBe(value);
          // The real plugin's data-only editor-menu callback uses the same proposal path.
          send({ type: "menu/request", request: 992, editor: snapshot });
          await vi.waitFor(() => expect(messages.some(m => m.type === "menu/result" && m.request === 992)).toBe(true));
          const menu = messages.find(m => m.type === "menu/result" && m.request === 992)!;
          const items = menu.items as { index: number; label: string }[];
          expect(items.map(item => item.label)).toContain("Highlight");
          const eraseItem = items.find(item => item.label === "Erase highlight")!;
          send({ ...menu, type: "menu/choose", index: eraseItem.index });
          await vi.waitFor(() => expect(messages.filter(m => m.type === "editor/propose")).toHaveLength(3));
          expect(messages.filter(m => m.type === "editor/propose").at(-1)).toMatchObject({ edits: erase.edits });
          expect(messages.filter(m => m.type === "error")).toEqual([]);
        }
      } finally {
        await window.happyDOM.close();
      }
    }, 30_000);
  }
});

describe.skipIf(!root || !pluginDirs.some((entry) => !isObsidian(join(root!, entry))))("Zotero plugin compatibility", () => {
  for (const name of pluginDirs.filter((entry) => !isObsidian(join(root!, entry)))) {
    it(`${name} starts without errors and shows what it registers`, async () => {
      const { window, messages, send } = await startPlugin(join(root!, name));
      try {
        const errors = messages.filter((message) => message.type === "error").map((message) => message.message);
        expect(errors).toEqual([]);
        expect(messages.some((message) => message.type === "ready")).toBe(true);
        const expected = EXPECTED[name] ?? {};
        if (expected.column) {
          expect(messages.filter((message) => message.type === "column").map((message) => message.label)).toContain(expected.column);
        }
        if (expected.menu) {
          expect((await itemMenu(send, messages)).some((label) => label.includes(expected.menu!))).toBe(true);
        }
        if (name === "pdf-translate") {
          expect(messages.filter((m) => m.type === "reader-events").some((m) =>
            (m.types as string[]).includes("renderTextSelectionPopup"))).toBe(true);
          expect(messages.some((m) => m.type === "settings-available")).toBe(true);
          send({ type: "settings" });
          const doc = window.document;
          await vi.waitFor(() => {
            expect(messages.filter((m) => m.type === "error").map((m) => m.message)).toEqual([]);
            expect(doc.querySelector("#zoteropdftranslate-sentenceServices select")).toBeTruthy();
          });
          const select = doc.querySelector("#zoteropdftranslate-sentenceServices select")! as unknown as { value: string };
          select.value = "google";
          doc.querySelector("#zoteropdftranslate-sentenceServices select")!.dispatchEvent(new window.Event("change", { bubbles: true }));
          expect(doc.getElementById("zoteropdftranslate-sentenceServices")!.getAttribute("value")).toBe("google");
          expect(messages.filter((m) => m.type === "error").map((m) => m.message)).toEqual([]);
          send({ type: "settings-close" });
          send({ type: "settings" });
          await vi.waitFor(() => expect((doc.querySelector("#zoteropdftranslate-sentenceServices select") as unknown as { value: string })?.value).toBe("google"));
          send({ type: "settings-close" });
          send({ type: "tabs", seq: 2, readers: [{ viewID: "pdf", attachmentKey: "PDF11111", parentItemKey: "AAAA1111" }], selectedViewID: "pdf" });
          send({ type: "reader-render", id: 910, live: true, event: "renderTextSelectionPopup", attachmentKey: "PDF11111", itemKey: "AAAA1111",
            params: { annotation: { text: "A long selected sentence for translation." } } });
          await vi.waitFor(() => expect(messages.some((m) => m.type === "reader-rendered" && m.id === 910 && String(m.html).includes("textarea"))).toBe(true));
          expect(messages.filter((m) => m.type === "error").map((m) => m.message)).toEqual([]);
          await vi.waitFor(() => expect(messages.some((m) => m.method === "requestUrl" &&
            String((m.args as { url: string }[])?.[0]?.url).startsWith("https://translate.google.com/"))).toBe(true));
          send({ type: "reader-release", id: 910 });
        }
        if (name === "zotero-actions-tags") {
          send({ type: "settings" });
          const doc = window.document;
          await vi.waitFor(() => expect(doc.querySelectorAll("tbody tr").length).toBeGreaterThan(0));
          const count = doc.querySelectorAll("tbody tr").length;
          expect(doc.querySelectorAll("th").length).toBeGreaterThan(3);
          const row = [...doc.querySelectorAll("tbody tr")].find((entry) => entry.textContent.includes("Add Unread"))!;
          row.dispatchEvent(new window.MouseEvent("click", { bubbles: true }));
          for (const id of ["edit", "remove", "duplicate"]) expect(doc.getElementById(`zoterotag-action-${id}`)!.hasAttribute("disabled")).toBe(false);
          row.dispatchEvent(new window.MouseEvent("dblclick", { bubbles: true }));
          await vi.waitFor(() => expect(doc.querySelector(".plugin-dialog-content")?.querySelector('[data-bind="name"]')).toBeTruthy());
          const editor = doc.querySelector(".plugin-dialog-content")!;
          const nameInput = editor.querySelector('[data-bind="name"]') as unknown as { value: string };
          nameInput.value = "AT compatibility saved";
          // Simulate selecting the event and operation with DOMString values.
          (editor.querySelector('[data-bind="event"]') as unknown as { value: string }).value = "1";
          (editor.querySelector('[data-bind="operation"]') as unknown as { value: string }).value = "1";
          (editor.querySelector('[data-bind="data"]') as unknown as { value: string }).value = "/at-compat";
          (editor.querySelector('[data-bind="menu"]') as unknown as { value: string }).value = "AT compatibility";
          const save = [...editor.querySelectorAll("button")].find((button) => /save/i.test(button.textContent));
          expect(save).toBeTruthy();
          save!.click();
          await vi.waitFor(() => expect(doc.querySelector(".plugin-dialog-layer")).toBeNull());
          await vi.waitFor(() => expect(doc.querySelector("tbody")!.textContent).toContain("AT compatibility saved"));
          expect(messages.some((m) => m.method === "prefs.save")).toBe(true);
          expect(doc.querySelector(".preference-pane")!.hasAttribute("hidden")).toBe(false);
          send({ type: "settings-close" });
          send({ type: "settings" });
          await vi.waitFor(() => expect(doc.querySelector("tbody")?.textContent).toContain("AT compatibility saved"));
          doc.getElementById("zoterotag-action-add")!.dispatchEvent(new window.MouseEvent("click", { bubbles: true }));
          await vi.waitFor(() => expect(doc.querySelector(".plugin-dialog-content")?.querySelector('[data-bind="name"]')).toBeTruthy());
          const added = doc.querySelector(".plugin-dialog-content")!;
          (added.querySelector('[data-bind="name"]') as unknown as { value: string }).value = "AT temporary";
          [...added.querySelectorAll("button")].find((button) => /save/i.test(button.textContent))!.click();
          await vi.waitFor(() => expect(doc.querySelector("tbody")!.textContent).toContain("AT temporary"));
          expect(doc.querySelectorAll("tbody tr")).toHaveLength(count + 1);
          [...doc.querySelectorAll("tbody tr")].find((row) => row.textContent.includes("AT temporary"))!.dispatchEvent(new window.MouseEvent("click", { bubbles: true }));
          doc.getElementById("zoterotag-action-remove")!.dispatchEvent(new window.MouseEvent("click", { bubbles: true }));
          expect(doc.querySelectorAll("tbody tr")).toHaveLength(count + 1);
          expect(doc.querySelector(".plugin-confirm-layer")!.textContent).toContain("delete the selected 1 action?");
          doc.querySelectorAll(".plugin-confirm-layer button")[1].dispatchEvent(new window.MouseEvent("click"));
          await vi.waitFor(() => expect(doc.querySelectorAll("tbody tr")).toHaveLength(count));
          doc.querySelector("th button")!.dispatchEvent(new window.MouseEvent("click"));
          const direction = doc.querySelector("th")!.getAttribute("aria-sort");
          doc.querySelector("th button")!.dispatchEvent(new window.MouseEvent("click"));
          expect(doc.querySelector("th")!.getAttribute("aria-sort")).not.toBe(direction);
          send({ type: "settings-close" });
          send({ type: "item-menu", id: 902, itemKeys: ["AAAA1111"] });
          await vi.waitFor(() => expect(messages.some((m) => m.type === "item-menu" && m.id === 902)).toBe(true));
          type MenuEntry = { label: string; index?: number; children?: MenuEntry[] };
          const menu = messages.find((m) => m.type === "item-menu" && m.id === 902)!.items as MenuEntry[];
          const trigger = menu.find((entry) => entry.label === "Trigger Action")!;
          const action = trigger.children!.find((entry) => entry.label === "AT compatibility")!;
          expect(action).toBeTruthy();
          send({ type: "item-command", menu: 902, index: action.index, path: [trigger.label, action.label], itemKeys: ["AAAA1111"] });
          await vi.waitFor(() => expect(messages.filter((m) => m.method === "items.update" || m.type === "error")).toContainEqual(expect.objectContaining({ method: "items.update", args: expect.arrayContaining([expect.arrayContaining(["/at-compat"])]) })));
          // Exercise the real A&T notifier and action dispatcher with annotation
          // grandchildren, including the built-in Add Tags operation.
          const zotero = (window as unknown as { Zotero: {
            ActionsTags: { api: { actionManager: {
              updateAction(action: Record<string, unknown>, key?: string): string;
              deleteAction(key: string): void;
              dispatchActionByEvent(event: number, args: Record<string, unknown>): Promise<unknown>;
            } } };
            Items: { getByLibraryAndKey(library: number, key: string): { id: number; removeTag(tag: string): void } };
          } }).Zotero;
          const manager = zotero.ActionsTags.api.actionManager;
          const dispatch = vi.spyOn(manager, "dispatchActionByEvent");
          const parentScript = (tag: string) => `const parent = Zotero.Items.getTopLevel([item])[0]; parent.addTag("${tag}"); await parent.saveTx();`;
          manager.updateAction({ name: "AN12 create parent", event: 4, operation: 4, data: parentScript("/annotated"), enabled: true }, "an12-create");
          manager.updateAction({ name: "AN12 colour parent", event: 11, operation: 4, data: parentScript("/recoloured"), enabled: true }, "an12-colour");
          const parent = { key: "AAAA1111", itemType: "journalArticle", tags: [], attachments: [{ key: "pdf" }] };
          const pdf = { key: "pdf", itemType: "attachment", parentItem: parent.key };
          let mark = { key: "annotation-uuid", itemType: "annotation", parentItem: "pdf", annotationType: "highlight",
            annotationText: "quote", annotationComment: "", annotationColor: "#ffd400", annotationPosition: '{"pageIndex":0,"rects":[[1,2,3,4]]}' };
          send({ type: "items", items: [parent, pdf], events: [] });
          expect(dispatch).not.toHaveBeenCalled();
          send({ type: "items", items: [parent, pdf, mark], events: [{ event: "add", objectType: "item", itemKeys: [mark.key] }] });
          await vi.waitFor(() => expect(messages.filter(m => m.method === "items.update")).toContainEqual(expect.objectContaining({ args: [parent.key, "{}", ["/annotated"]] })));
          send({ type: "items", items: [parent, pdf, mark], events: [] });
          expect(dispatch.mock.calls.filter(([event]) => event === 4)).toHaveLength(1);
          await vi.waitFor(() => expect(dispatch.mock.calls.filter(([event]) => event === 6)).toHaveLength(1));
          mark = { ...mark, annotationComment: "Comment only" };
          send({ type: "items", items: [parent, pdf, mark], events: [{ event: "modify", objectType: "item", itemKeys: [mark.key], changed: { [mark.key]: ["annotationComment"] } }] });
          expect(dispatch.mock.calls.filter(([event]) => event === 11)).toHaveLength(0);
          mark = { ...mark, annotationColor: "#abcdef" };
          send({ type: "items", items: [parent, pdf, mark], events: [{ event: "modify", objectType: "item", itemKeys: [mark.key], changed: { [mark.key]: ["annotationColor"] } }] });
          await vi.waitFor(() => expect(messages.filter(m => m.method === "items.update")).toContainEqual(expect.objectContaining({ args: [parent.key, "{}", ["/recoloured"]] })));
          expect(dispatch.mock.calls.filter(([event]) => event === 11)).toHaveLength(1);
          const direct = manager.updateAction({ name: "AN3 child tag", event: 4, operation: 1, data: "#auto", enabled: true }, "an3-tag");
          const comment = manager.updateAction({ name: "AN3 child comment", event: 4, operation: 4, data: "item.annotationComment = '  translated\\n'; await item.saveTx();", enabled: true }, "an3-comment");
          send({ type: "items", items: [parent, pdf, mark, { ...mark, key: "second" }], events: [{ event: "add", objectType: "item", itemKeys: ["second"] }] });
          await vi.waitFor(() => expect(messages.filter(m => m.method === "items.update")).toContainEqual(expect.objectContaining({ args: ["second", "{}", ["#auto"]] })));
          await vi.waitFor(() => expect(messages.filter(m => m.method === "items.update")).toContainEqual(expect.objectContaining({ args: ["second", JSON.stringify({ annotationComment: "  translated\n" }), null] })));
          manager.deleteAction(direct);
          manager.deleteAction(comment);
          dispatch.mockRestore();
          expect(messages.filter(m => m.type === "error")).toHaveLength(0);
        }
      } finally {
        await window.happyDOM.close();
      }
    }, 30_000);
  }
});
