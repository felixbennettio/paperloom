// Editing commands for Markdown notes, as pure functions of the text and the
// selection, so the toolbar and shortcuts do the same thing everywhere.

/** Replace `from…to` with `insert`, then select `selectFrom…selectTo`. */
export interface Edit {
  from: number;
  to: number;
  insert: string;
  selectFrom: number;
  selectTo: number;
}

/** Wraps the selection in markers, or unwraps it if it already is. */
export function toggleWrap(doc: string, from: number, to: number, before: string, after = before): Edit {
  const selected = doc.slice(from, to);
  if (selected.length >= before.length + after.length && selected.startsWith(before) && selected.endsWith(after)) {
    const inner = selected.slice(before.length, selected.length - after.length);
    return { from, to, insert: inner, selectFrom: from, selectTo: from + inner.length };
  }
  if (doc.slice(from - before.length, from) === before && doc.slice(to, to + after.length) === after) {
    const start = from - before.length;
    return { from: start, to: to + after.length, insert: selected, selectFrom: start, selectTo: start + selected.length };
  }
  const start = from + before.length;
  return { from, to, insert: before + selected + after, selectFrom: start, selectTo: start + selected.length };
}

/**
 * Puts a prefix (`- `, `1. `, `> `, `- [ ] `, `## `) before every line the
 * selection touches, or takes it away if all of them have it. A heading
 * prefix replaces another heading level.
 */
export function toggleLinePrefix(doc: string, from: number, to: number, prefix: string): Edit {
  const start = doc.lastIndexOf("\n", from - 1) + 1;
  const newline = doc.indexOf("\n", to);
  const end = newline < 0 ? doc.length : newline;
  const lines = doc.slice(start, end).split("\n");
  const heading = /^#{1,6} $/.test(prefix);
  const all = lines.every((line) => line.startsWith(prefix));
  const insert = lines
    .map((line) => {
      if (all) return line.slice(prefix.length);
      if (heading) return prefix + line.replace(/^#{1,6} /, "");
      return prefix + line;
    })
    .join("\n");
  return { from: start, to: end, insert, selectFrom: start, selectTo: start + insert.length };
}

/**
 * Colors the selection with inline HTML, which Obsidian, Typora and GitHub
 * all show: `<span style="color: …">` or `<mark style="background: …">`.
 */
export function colorText(doc: string, from: number, to: number, color: string, background: boolean): Edit {
  const selected = doc.slice(from, to);
  const open = background ? `<mark style="background: ${color}">` : `<span style="color: ${color}">`;
  const close = background ? "</mark>" : "</span>";
  const start = from + open.length;
  return { from, to, insert: open + selected + close, selectFrom: start, selectTo: start + selected.length };
}

/** Inserts text in place of the selection; the cursor goes `cursor` characters in. */
export function insertText(from: number, to: number, text: string, cursor = text.length): Edit {
  return { from, to, insert: text, selectFrom: from + cursor, selectTo: from + cursor };
}

/** `[[selection]]`, with the cursor inside when nothing was selected. */
export function wikiLink(doc: string, from: number, to: number): Edit {
  const selected = doc.slice(from, to);
  const insert = `[[${selected}]]`;
  return { from, to, insert, selectFrom: from + 2, selectTo: from + 2 + selected.length };
}

export const TABLE = "| Column | Column |\n| --- | --- |\n|  |  |\n";

export const TEXT_COLORS = ["#e03e3e", "#d9730d", "#0f7b6c", "#0b6e99", "#6940a5"];
export const BACKGROUND_COLORS = ["#fff3a3", "#ffd8b8", "#d3f5d0", "#d7e9fb", "#f5d9f0"];
