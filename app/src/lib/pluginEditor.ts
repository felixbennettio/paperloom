// The only visual data allowed out of an editor sandbox. Never adopt plugin DOM,
// HTML, stylesheets, callbacks, tags or event handlers into the app.
import { StateEffect, StateField, type EditorState } from "@codemirror/state";
import { Decoration, EditorView, WidgetType, type DecorationSet } from "@codemirror/view";
import type { EditorSnapshot } from "./plugins";

export interface TextSpan { from: number; to: number; attributes: Record<string, string> }
export interface PluginDecoration extends TextSpan {
  kind: "mark" | "line" | "replace" | "widget";
  text?: string;
  spans?: TextSpan[];
  partial?: true;
}
export interface PluginDecorations {
  plugin: string; session: string; generation: number; revision: number; contextRevision: number;
  decorations: PluginDecoration[];
}
const namedColors = new Set("aliceblue antiquewhite aqua aquamarine azure beige bisque black blanchedalmond blue blueviolet brown burlywood cadetblue chartreuse chocolate coral cornflowerblue cornsilk crimson cyan darkblue darkcyan darkgoldenrod darkgray darkgreen darkgrey darkkhaki darkmagenta darkolivegreen darkorange darkorchid darkred darksalmon darkseagreen darkslateblue darkslategray darkslategrey darkturquoise darkviolet deeppink deepskyblue dimgray dimgrey dodgerblue firebrick floralwhite forestgreen fuchsia gainsboro ghostwhite gold goldenrod gray green greenyellow grey honeydew hotpink indianred indigo ivory khaki lavender lavenderblush lawngreen lemonchiffon lightblue lightcoral lightcyan lightgoldenrodyellow lightgray lightgreen lightgrey lightpink lightsalmon lightseagreen lightskyblue lightslategray lightslategrey lightsteelblue lightyellow lime limegreen linen magenta maroon mediumaquamarine mediumblue mediumorchid mediumpurple mediumseagreen mediumslateblue mediumspringgreen mediumturquoise mediumvioletred midnightblue mintcream mistyrose moccasin navajowhite navy oldlace olive olivedrab orange orangered orchid palegoldenrod palegreen paleturquoise palevioletred papayawhip peachpuff peru pink plum powderblue purple rebeccapurple red rosybrown royalblue saddlebrown salmon sandybrown seagreen seashell sienna silver skyblue slateblue slategray slategrey snow springgreen steelblue tan teal thistle tomato transparent turquoise violet wheat white whitesmoke yellow yellowgreen".split(" "));
const classNames = /^(?:[a-zA-Z_-][\w-]*)(?:\s+[a-zA-Z_-][\w-]*)*$/;
// Deliberately no broader than core/crates/vault/src/note_html.rs's inline CSS.
export function decorationStyle(style: string): string {
  return style.split(";").flatMap(declaration => {
    const colon = declaration.indexOf(":");
    if (colon < 0) return [];
    const property = declaration.slice(0, colon).trim().toLowerCase(), value = declaration.slice(colon + 1).trim();
    const rgb = /^(rgb|rgba)\(([^)]+)\)$/i.exec(value);
    const components = rgb?.[2].split(",").map(v => v.trim());
    const color = /^#(?:[\da-f]{3,4}|[\da-f]{6}|[\da-f]{8})$/i.test(value) || namedColors.has(value.toLowerCase()) ||
      !!(rgb && components && components.length === (rgb[1].toLowerCase() === "rgba" ? 4 : 3) &&
        components.every((v, i) => /^(?:\d+(?:\.\d+)?|\.\d+)$/.test(v) && Number(v) <= (i === 3 ? 1 : 255)));
    const words = value.toLowerCase().split(/\s+/);
    const valid = /^(?:color|background|background-color)$/.test(property) ? color : property === "text-decoration" &&
      words.every(word => /^(?:none|underline|overline|line-through|solid|double|dotted|dashed|wavy)$/.test(word)) && (!words.includes("none") || words.length === 1);
    return valid ? [`${property}:${value}`] : [];
  }).join(";");
}
export function decorationAttributes(input: unknown): Record<string, string> {
  const result: Record<string, string> = {};
  if (!input || typeof input !== "object" || Array.isArray(input)) return result;
  for (const [name, value] of Object.entries(input).slice(0, 30)) {
    if (typeof value !== "string" || value.length > 2000) continue;
    if (name === "class" && classNames.test(value)) result[name] = value;
    else if (name === "title" || /^data-[a-z][a-z0-9-]*$/.test(name)) result[name] = value;
    else if (name === "style") { const safe = decorationStyle(value); if (safe) result[name] = safe; }
  }
  return result;
}
export function validatePluginDecorations(plugin: string, input: Record<string, unknown>, editor: EditorSnapshot | null): PluginDecorations | null {
  if (!editor || input.session !== editor.session || input.generation !== editor.generation || input.revision !== editor.revision ||
    input.contextRevision !== editor.contextRevision || !Array.isArray(input.decorations) || input.decorations.length > 10000) return null;
  const decorations: PluginDecoration[] = [];
  let payload = 0;
  for (const raw of input.decorations) {
    if (!raw || !["mark", "line", "replace", "widget"].includes(raw.kind) || !Number.isSafeInteger(raw.from) || !Number.isSafeInteger(raw.to) ||
      raw.from < 0 || raw.to < raw.from || raw.to > editor.value.length || ((raw.kind === "line" || raw.kind === "widget") && raw.from !== raw.to) ||
      (raw.kind === "mark" && raw.from === raw.to)) return null;
    const attributes = decorationAttributes(raw.attributes);
    payload += JSON.stringify(attributes).length;
    const decoration: PluginDecoration = { kind: raw.kind, from: raw.from, to: raw.to, attributes };
    if (raw.kind === "replace" || raw.kind === "widget") {
      if (typeof raw.text !== "string" || raw.text.length > 20000 || !Array.isArray(raw.spans) || raw.spans.length > 500) return null;
      decoration.text = raw.text; decoration.partial = true; decoration.spans = [];
      payload += raw.text.length;
      for (const span of raw.spans) {
        if (!span || !Number.isSafeInteger(span.from) || !Number.isSafeInteger(span.to) || span.from < 0 || span.to < span.from || span.to > raw.text.length) return null;
        const attributes = decorationAttributes(span.attributes);
        payload += JSON.stringify(attributes).length;
        decoration.spans.push({ from: span.from, to: span.to, attributes });
      }
    }
    if (payload > 1_000_000) return null;
    decorations.push(decoration);
  }
  return { plugin, session: editor.session, generation: editor.generation, revision: editor.revision, contextRevision: editor.contextRevision, decorations };
}
const results = new Map<string, PluginDecorations>();
const listeners = new Set<() => void>();
export function publishPluginDecorations(plugin: string, result: PluginDecorations | null) {
  if (result) results.set(plugin, result); else results.delete(plugin);
  for (const listener of listeners) listener();
}
export const subscribePluginDecorations = (listener: () => void) => { listeners.add(listener); return () => { listeners.delete(listener); }; };
export const currentPluginDecorations = () => [...results.values()];
export const setPluginDecorations = StateEffect.define<{ results: PluginDecorations[]; live: boolean }>();

class StaticWidget extends WidgetType {
  constructor(readonly plugin: string, readonly decoration: PluginDecoration) { super(); }
  eq(other: StaticWidget) { return this.plugin === other.plugin && JSON.stringify(this.decoration) === JSON.stringify(other.decoration); }
  toDOM() {
    const root = document.createElement("span");
    root.className = "plugin-editor-snapshot";
    root.dataset.plugin = this.plugin;
    root.dataset.partial = "true";
    const { text = "", spans = [] } = this.decoration;
    const cuts = [...new Set([0, text.length, ...spans.flatMap(s => [s.from, s.to])])].sort((a, b) => a - b);
    for (let i = 0; i < cuts.length - 1; i++) {
      const segment = document.createElement("span");
      segment.textContent = text.slice(cuts[i], cuts[i + 1]);
      const attributes: Record<string, string> = {};
      for (const span of spans.filter(s => s.from <= cuts[i] && s.to >= cuts[i + 1])) {
        for (const [name, value] of Object.entries(span.attributes)) {
          if (!(name in attributes) && Object.keys(attributes).length >= 30) continue;
          attributes[name] = (attributes[name] && (name === "class" || name === "style") ?
            `${attributes[name]}${name === "class" ? " " : ";"}${value}` : value).slice(0, 2000);
        }
      }
      for (const [name, value] of Object.entries(attributes)) segment.setAttribute(name, value);
      root.appendChild(segment);
    }
    for (const [name, value] of Object.entries(this.decoration.attributes)) root.setAttribute(name, value);
    return root;
  }
  ignoreEvent() { return false; }
}
interface Visuals { decorations: DecorationSet; replacements: { from: number; to: number }[] }
function build(state: EditorState, input: { results: PluginDecorations[]; live: boolean }): Visuals {
  const ranges = [], replacements: Visuals["replacements"] = [];
  if (input.live) for (const result of input.results) for (const d of result.decorations) {
    if (d.to > state.doc.length || state.selection.ranges.some(r => state.doc.lineAt(r.from).from <= d.to && state.doc.lineAt(r.to).to >= d.from)) continue;
    if (d.kind === "line") {
      if (state.doc.lineAt(d.from).from === d.from) ranges.push(Decoration.line({ attributes: d.attributes }).range(d.from));
    } else if (d.kind === "mark") ranges.push(Decoration.mark({ attributes: d.attributes }).range(d.from, d.to));
    else {
      if (replacements.some(r => d.from < r.to && d.to > r.from)) continue;
      const widget = new StaticWidget(result.plugin, d);
      ranges.push((d.kind === "replace" ? Decoration.replace({ widget }) : Decoration.widget({ widget })).range(d.from, d.to));
      if (d.kind === "replace") replacements.push({ from: d.from, to: d.to });
    }
  }
  return { decorations: Decoration.set(ranges, true), replacements };
}
export const pluginEditor = StateField.define<Visuals>({
  create: () => ({ decorations: Decoration.none, replacements: [] }),
  update(value, tr) {
    // Remove stale visuals synchronously before the next frame's response.
    if (tr.docChanged || tr.selection) value = { decorations: Decoration.none, replacements: [] };
    for (const effect of tr.effects) if (effect.is(setPluginDecorations)) value = build(tr.state, effect.value);
    return value;
  },
  provide: field => EditorView.decorations.from(field, value => value.decorations),
});
export const pluginReplacementRanges = (state: EditorState) => state.field(pluginEditor, false)?.replacements ?? [];
