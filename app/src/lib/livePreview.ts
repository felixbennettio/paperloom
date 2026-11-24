// Live preview for the note editor: Markdown marks (#, **, `, ~~) are
// hidden and [[links]] shown as links on every line except the ones being
// edited, and headings are drawn at their size in both modes.

import { autocompletion, type Completion, type CompletionContext } from "@codemirror/autocomplete";
import { HighlightStyle, syntaxTree } from "@codemirror/language";
import { RangeSetBuilder } from "@codemirror/state";
import { Decoration, type DecorationSet, EditorView, ViewPlugin, type ViewUpdate, WidgetType } from "@codemirror/view";
import { tags } from "@lezer/highlight";
import { pluginReplacementRanges } from "./pluginEditor";
import type { CitationCompletion, LinkCompletion } from "./api";

export const noteHighlight = HighlightStyle.define([
  { tag: tags.strong, fontWeight: "700" },
  { tag: tags.emphasis, fontStyle: "italic" },
  { tag: tags.strikethrough, textDecoration: "line-through" },
  { tag: tags.link, color: "var(--accent)" },
  { tag: tags.url, color: "var(--muted)" },
  { tag: tags.monospace, fontFamily: "ui-monospace, SFMono-Regular, Menlo, Consolas, monospace" },
  { tag: tags.quote, color: "var(--muted)" },
  { tag: [tags.processingInstruction, tags.meta], color: "var(--muted)" },
]);

const hidden = Decoration.replace({});

class LinkWidget extends WidgetType {
  constructor(
    readonly label: string,
    readonly target: string,
    readonly open: (target: string) => void,
  ) {
    super();
  }

  eq(other: LinkWidget): boolean {
    return other.label === this.label && other.target === this.target;
  }

  toDOM(): HTMLElement {
    const link = document.createElement("a");
    link.className = "cm-wikilink";
    link.textContent = this.label;
    link.addEventListener("mousedown", (event) => {
      event.preventDefault();
      this.open(this.target);
    });
    return link;
  }

  ignoreEvent(): boolean {
    return false;
  }
}

interface Pending {
  from: number;
  to: number;
  decoration: Decoration;
}

function decorations(view: EditorView, live: boolean, open: (target: string) => void): DecorationSet {
  const { state } = view;
  const editing = new Set<number>();
  for (const range of state.selection.ranges) {
    for (let n = state.doc.lineAt(range.from).number; n <= state.doc.lineAt(range.to).number; n++) editing.add(n);
  }
  const pending: Pending[] = [];
  const links: { from: number; to: number }[] = [];
  for (const { from, to } of view.visibleRanges) {
    if (live) {
      const text = state.doc.sliceString(from, to);
      for (const match of text.matchAll(/!?\[\[([^\]\n]+?)\]\]/g)) {
        const start = from + (match.index ?? 0);
        const end = start + match[0].length;
        if (editing.has(state.doc.lineAt(start).number)) continue;
        const [targetPart, alias] = match[1].split("|");
        const target = targetPart.replace(/\\$/, "");
        const label = alias ?? (target.startsWith("#") ? target.slice(1) : target.replace(/#/, " › "));
        links.push({ from: start, to: end });
        pending.push({ from: start, to: end, decoration: Decoration.replace({ widget: new LinkWidget(label, target, open) }) });
      }
    }
    syntaxTree(state).iterate({
      from,
      to,
      enter: (node) => {
        const heading = /^ATXHeading(\d)$/.exec(node.name);
        const line = state.doc.lineAt(node.from);
        if (heading) {
          pending.push({ from: line.from, to: line.from, decoration: Decoration.line({ class: `cm-heading cm-heading-${heading[1]}` }) });
        }
        if (!live || editing.has(line.number) || links.some((l) => node.from < l.to && node.to > l.from)) return;
        if (node.name === "HeaderMark") {
          pending.push({ from: node.from, to: Math.min(node.to + 1, line.to), decoration: hidden });
        } else if (
          node.name === "EmphasisMark" ||
          node.name === "StrikethroughMark" ||
          (node.name === "CodeMark" && node.node.parent?.name === "InlineCode")
        ) {
          pending.push({ from: node.from, to: node.to, decoration: hidden });
        }
      },
    });
  }
  pending.sort((a, b) => a.from - b.from || a.decoration.startSide - b.decoration.startSide);
  const builder = new RangeSetBuilder<Decoration>();
  let last = -1;
  const pluginRanges = pluginReplacementRanges(state);
  for (const { from, to, decoration } of pending) {
    if (from !== to && pluginRanges.some(r => from < r.to && to > r.from)) continue;
    if (from < last && from !== to) continue; // never overlap hidden ranges
    builder.add(from, to, decoration);
    if (from !== to) last = to;
  }
  return builder.finish();
}

/** Headings at their size; with `live`, marks hidden and links shown as links. */
export function notePreview(live: boolean, open: (target: string) => void) {
  return ViewPlugin.fromClass(
    class {
      decorations: DecorationSet;

      constructor(view: EditorView) {
        this.decorations = decorations(view, live, open);
      }

      update(update: ViewUpdate) {
        if (update.docChanged || update.selectionSet || update.viewportChanged || update.transactions.some(tr => tr.effects.length)) {
          this.decorations = decorations(update.view, live, open);
        }
      }
    },
    { decorations: (plugin) => plugin.decorations },
  );
}

/** Completes `[[` with note names, and `[@` with library items. */
export function linkCompletion(
  find: (query: string) => Promise<LinkCompletion[]>,
  cite?: (query: string) => Promise<CitationCompletion[]>,
) {
  return autocompletion({
    override: [
      async (context: CompletionContext) => {
        if (!cite) return null;
        // `[@ka`, `[see @a; @b`: the word after the last `@`.
        const typed = context.matchBefore(/\[[^\]\n]*@[^\s\];,@]*/);
        if (!typed) return null;
        const at = typed.text.lastIndexOf("@");
        const query = typed.text.slice(at + 1);
        const found = await cite(query.replace(/[-_]/g, " "));
        // `[[@key]]` links the item, as `[[note]]` links a note; `[@key]`
        // cites it, and more citations may follow inside the brackets.
        const link = typed.text.slice(0, at) === "[[";
        const options: Completion[] = found.map((item) => ({
          label: item.key,
          detail: item.cite,
          info: item.title,
          apply: (view, _completion, from, to) => {
            const close = link ? "]]" : "]";
            const closed = view.state.sliceDoc(to, to + close.length) === close;
            const insert = closed ? item.key : `${item.key}${close}`;
            const cursor = from + item.key.length + (link ? close.length : 0);
            view.dispatch({ changes: { from, to, insert }, selection: { anchor: cursor } });
          },
        }));
        return { from: typed.from + at + 1, options, filter: false };
      },
      async (context: CompletionContext) => {
        const typed = context.matchBefore(/\[\[[^\]\n|#]*/);
        if (!typed) return null;
        const found = await find(typed.text.slice(2));
        const options: Completion[] = found.map((note) => ({
          label: note.insert,
          detail: note.path === `${note.insert}.md` ? undefined : note.path,
          apply: (view, _completion, from, to) => {
            const closed = view.state.sliceDoc(to, to + 2) === "]]";
            const insert = closed ? note.insert : `${note.insert}]]`;
            view.dispatch({ changes: { from, to, insert }, selection: { anchor: from + insert.length + (closed ? 2 : 0) } });
          },
        }));
        return { from: typed.from + 2, options, filter: false };
      },
    ],
  });
}
