import { readFileSync } from "node:fs";
import { Window } from "happy-dom";
import { describe, it, expect, vi } from "vitest";
import { EditorState } from "@codemirror/state";
import { decorationAttributes, pluginEditor, pluginReplacementRanges, setPluginDecorations, validatePluginDecorations } from "./pluginEditor";
import type { EditorSnapshot } from "./plugins";

const bundle = readFileSync(new URL("../../../core/plugin-runtime/cm6.js", import.meta.url), "utf8");
describe("sandbox CM bundle", () => {
  it("uses the app lockfile versions and one identity for CM extensions", async () => {
    const window = new Window();
    try {
      window.eval(bundle);
      const lock = JSON.parse(readFileSync(new URL("../../package-lock.json", import.meta.url), "utf8"));
      const versions = window.eval("PaperloomCM6.versions");
      expect(Object.keys(versions).length).toBeGreaterThanOrEqual(12);
      for (const [name, version] of Object.entries(versions)) expect(version, `Rebuild with npm run build:plugin-cm: ${name}`).toBe(lock.packages[`node_modules/${name}`].version);
      expect(window.eval(`(() => { const cm = PaperloomCM6, s = cm.modules['@codemirror/state'];
        return s.EditorState.create({extensions:[cm.markdown.markdown(), cm.editorInfoField, cm.editorLivePreviewField]}).field(cm.editorLivePreviewField) === false; })()`)).toBe(true);
    } finally { await window.happyDOM.close(); }
  });
  it("maps Dataview's code-block names and inline-code content token boundaries", async () => {
    const window = new Window();
    try {
      window.eval(bundle);
      const nodes = window.eval(`(() => { const cm = PaperloomCM6, lang = cm.modules['@codemirror/language'];
        const state = cm.modules['@codemirror/state'].EditorState.create({doc:'[key:: value]\\n\\n\x60\x60\x60\\n[field:: no]\\n\x60\x60\x60\\n\\n\x60= 1 + 2\x60', extensions: cm.markdown.markdown()});
        const nodes = []; lang.syntaxTree(state).iterate({enter(ref) { nodes.push({name:ref.name, text:state.sliceDoc(ref.from, ref.to), token:ref.type.prop(lang.tokenClassNodeProp)}); }}); return nodes; })()`);
      expect(nodes).toContainEqual(expect.objectContaining({ name: "HyperMD-codeblock", text: expect.stringContaining("[field:: no]") }));
      expect(nodes).toContainEqual(expect.objectContaining({ text: "= 1 + 2", token: "inline-code" }));
    } finally { await window.happyDOM.close(); }
  });
});
const editor: EditorSnapshot = { protocol: 1, session: "s", generation: 1, revision: 2, contextRevision: 3,
  value: "status:: done\nnext", ranges: [{ anchor: 17, head: 17 }], mainIndex: 0, mode: "live", focused: true, composing: false };
const message = { ...editor, decorations: [{ kind: "replace", from: 0, to: 13, text: "statusdone", attributes: {},
  spans: [{ from: 0, to: 6, attributes: { class: "inline-field-key", onclick: "bad()" } }] }] };
describe("trusted plugin decorations", () => {
  it("bounds data and refuses stale, malformed and oversized exports", () => {
    expect(validatePluginDecorations("dv", message, editor)?.decorations[0].spans?.[0].attributes).toEqual({ class: "inline-field-key" });
    for (const change of [{ session: "old" }, { generation: 2 }, { revision: 1 }, { contextRevision: 2 },
      { decorations: [{ ...message.decorations[0], to: 999 }] }, { decorations: [{ ...message.decorations[0], spans: [{ from: 0, to: 100 }] }] },
      { decorations: Array(10001).fill(message.decorations[0]) }]) expect(validatePluginDecorations("dv", { ...message, ...change }, editor)).toBeNull();
  });
  it("restricts CSS to the vault inline HTML policy", () => {
    expect(decorationAttributes({ class: "dv field", title: "tip", "data-dv-key": "status", id: "app", onclick: "bad()",
      style: "position:fixed;background:url(https://bad);color:RebeccaPurple;text-decoration:underline wavy;background-color:rgba(1,2,3,.5);width:200px" }))
      .toEqual({ class: "dv field", title: "tip", "data-dv-key": "status", style: "color:RebeccaPurple;text-decoration:underline wavy;background-color:rgba(1,2,3,.5)" });
    expect(decorationAttributes({ class: 'bad" onmouseover="bad', style: "color:currentColor;background:var(--x);color:rgb(999,0,0);color:hsl(1,2%,3%)" })).toEqual({});
  });
  it("removes replacements immediately on cursor entry, edits and source mode", () => {
    const result = validatePluginDecorations("dv", message, editor)!;
    let state = EditorState.create({ doc: editor.value, selection: { anchor: 17 }, extensions: [pluginEditor] });
    const apply = () => { state = state.update({ effects: setPluginDecorations.of({ results: [result], live: true }) }).state; };
    apply(); expect(pluginReplacementRanges(state)).toEqual([{ from: 0, to: 13 }]);
    state = state.update({ selection: { anchor: 2 } }).state;
    expect(pluginReplacementRanges(state)).toEqual([]);
    apply(); expect(pluginReplacementRanges(state)).toEqual([]);
    state = state.update({ selection: { anchor: 17 } }).state;
    apply();
    state = state.update({ effects: setPluginDecorations.of({ results: [result], live: false }) }).state;
    expect(pluginReplacementRanges(state)).toEqual([]);
    apply(); state = state.update({ changes: { from: 0, insert: "x" } }).state;
    expect(pluginReplacementRanges(state)).toEqual([]);
  });
});

describe("real sandbox editor mirrors", () => {
  it("exports StateField/ViewPlugin providers, refreshes async text, reconfigures retained arrays and cleans up", async () => {
    const window = new Window({ url: "http://plugin.localhost/host.html" });
    const messages: Record<string, any>[] = [];
    const parent = { postMessage: (message: Record<string, any>) => messages.push(message) };
    Object.defineProperty(window, "parent", { value: parent });
    const send = (data: object) => window.dispatchEvent(new window.MessageEvent("message", { source: parent as never, data }));
    try {
      window.eval(bundle);
      window.eval(readFileSync(new URL("../../../core/plugin-runtime/obsidian.js", import.meta.url), "utf8"));
      send({ type: "start", manifest: { id: "fixture", name: "Fixture", version: "1" }, files: [], code: { main: `
        const {Plugin, editorInfoField, editorLivePreviewField, MarkdownView} = require('obsidian');
        const {StateField} = require('@codemirror/state');
        const {ViewPlugin, Decoration, WidgetType, EditorView} = require('@codemirror/view');
        module.exports = class extends Plugin { onload() {
          window.fixture = this;
          this.addCommand({id:'source-bound', callback: () => { window.commandRead = this.app.workspace.activeLeaf.view.editor.getValue(); }});
          class Widget extends WidgetType { toDOM() {
            const span = this.element || (this.element = document.createElement('span')); span.className = 'fixture';
            span.textContent = 'loading'; setTimeout(() => { span.textContent = 'async ready'; }, 40); return span;
          } }
          const field = StateField.define({create: () => Decoration.set([Decoration.line({class:'fixture-line'}).range(0), Decoration.mark({class:'fixture-mark'}).range(0,3)]), update: v => v, provide: f => EditorView.decorations.from(f)});
          const visual = ViewPlugin.fromClass(class { constructor(view) {
            if (!(view.state.field(editorInfoField).view instanceof MarkdownView) || !view.state.field(editorLivePreviewField)) throw Error('missing editor context');
            this.decorations = Decoration.set([Decoration.replace({widget:new Widget()}).range(4,7)]);
          } }, {decorations: p => p.decorations});
          this.extensions = [field, visual]; this.registerEditorExtension(this.extensions);
        } };` } });
      await new Promise(resolve => setTimeout(resolve, 10));
      const snapshot = { ...editor, session: "mirror", path: "fixture.md", value: "one two\nnext", ranges: [{ anchor: 12, head: 12 }] };
      send({ type: "editor/open", editor: snapshot });
      const latest = () => messages.filter(m => m.type === "decoration/result").at(-1)!;
      await vi.waitFor(() => expect(latest()?.decorations.some((d: any) => d.text === "async ready")).toBe(true));
      expect(messages.filter(m => m.type === "error")).toEqual([]);
      expect(latest().decorations).toContainEqual(expect.objectContaining({ kind: "mark", from: 0, to: 3 }));
      expect(latest().decorations).toContainEqual(expect.objectContaining({ kind: "line", from: 0, to: 0 }));
      expect(window.eval("fixture.app.workspace.activeLeaf.view.editor.cm.state.doc.toString()")).toBe(snapshot.value);
      expect(window.eval("fixture.app.workspace.activeLeaf.view.editor.cm.contentDOM.textContent")).toContain("async ready");
      send({ type: "command/run", id: "fixture:source-bound", editor: { ...snapshot, value: "old context", ranges: [{ anchor: 0, head: 0 }] } });
      await vi.waitFor(() => expect(window.eval("commandRead")).toBe("old context"));
      window.eval("fixture.extensions.length = 0; fixture.app.workspace.updateOptions()");
      await vi.waitFor(() => expect(latest().decorations).toEqual([]));
      send({ type: "editor/close", session: snapshot.session, generation: snapshot.generation });
      expect(window.document.querySelector(".paperloom-editor-mirror")).toBeNull();
      send({ type: "stop" });
    } finally { await window.happyDOM.close(); }
  });
});
