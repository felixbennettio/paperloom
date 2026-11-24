import { defaultKeymap, history, historyKeymap, indentWithTab, isolateHistory } from "@codemirror/commands";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { syntaxHighlighting } from "@codemirror/language";
import { highlightSelectionMatches, searchKeymap } from "@codemirror/search";
import { ChangeSet, Compartment, EditorSelection, EditorState, Prec, StateEffect, StateField, Transaction, type TransactionSpec } from "@codemirror/state";
import { drawSelection, EditorView, keymap, placeholder, showTooltip, type Tooltip } from "@codemirror/view";
import { useEffect, useImperativeHandle, useRef, useState, type Ref } from "react";
import { useI18n } from "../i18n";
import { pluginHost, type EditorMenu, type EditorProposal, type EditorSnapshot, type PluginSuggestion } from "../lib/plugins";
import { currentPluginDecorations, pluginEditor, setPluginDecorations, subscribePluginDecorations } from "../lib/pluginEditor";
import { createPortal } from "react-dom";
import { MenuButton } from "./MenuButton";
import type { CitationCompletion, LinkCompletion } from "../lib/api";
import { linkCompletion, notePreview, noteHighlight } from "../lib/livePreview";
import {
  BACKGROUND_COLORS,
  colorText,
  insertText,
  TABLE,
  TEXT_COLORS,
  toggleLinePrefix,
  toggleWrap,
  wikiLink,
  type Edit,
} from "../lib/markdown";

export interface NoteEditorHandle {
  /** Moves the cursor to a line (from 0) and shows it. */
  goToLine(line: number): void;
  /** Puts text where the cursor is (in place of a selection). */
  insert(text: string): void;
  /** The text and the selection, as offsets into it. */
  state(): EditorSnapshot | null;
  apply(proposal: EditorProposal): boolean;
  /** Puts in new text for the whole note. */
  setValue(text: string): void;
}

interface Props {
  initialText: string;
  live: boolean;
  onChange: (text: string) => void;
  onEditorUpdate?: (origin: string, userEvent?: string, changes?: EditorProposal["edits"]) => void;
  onOpenLink: (target: string) => void;
  complete: (query: string) => Promise<LinkCompletion[]>;
  /** Library items for `[@` citations. */
  cite?: (query: string) => Promise<CitationCompletion[]>;
  ref?: Ref<NoteEditorHandle>;
}

type Command = (doc: string, from: number, to: number) => Edit;

const bold: Command = (doc, from, to) => toggleWrap(doc, from, to, "**");
const italic: Command = (doc, from, to) => toggleWrap(doc, from, to, "*");

let nextSession = 0;

/** Validate completely before dispatch. Sequential edits compose into one history entry. */
export function pluginTransaction(state: EditorState, snapshot: EditorSnapshot, proposal: EditorProposal): TransactionSpec | null {
  if (!proposal || typeof proposal.operation !== "string" || proposal.operation.length > 200 ||
    proposal.session !== snapshot.session || proposal.generation !== snapshot.generation ||
    proposal.baseRevision !== snapshot.revision || proposal.baseContextRevision !== snapshot.contextRevision || snapshot.composing ||
    !Array.isArray(proposal.edits) || proposal.edits.length > 1000 || !Array.isArray(proposal.ranges) ||
    !proposal.ranges.length || proposal.ranges.length > 1000 || !Number.isSafeInteger(proposal.mainIndex) ||
    proposal.mainIndex < 0 || proposal.mainIndex >= proposal.ranges.length) return null;
  let changes = ChangeSet.empty(state.doc.length);
  let length = state.doc.length;
  let payload = 0;
  for (const edit of proposal.edits) {
    if (!edit || !Number.isSafeInteger(edit.from) || !Number.isSafeInteger(edit.to) || edit.from < 0 || edit.to < edit.from ||
      edit.to > length || typeof edit.insert !== "string" || (payload += edit.insert.length) > 10_000_000) return null;
    const next = ChangeSet.of(edit, length);
    changes = changes.compose(next);
    length = next.newLength;
  }
  for (const range of proposal.ranges) {
    if (!range || !Number.isSafeInteger(range.anchor) || !Number.isSafeInteger(range.head) ||
      range.anchor < 0 || range.head < 0 || range.anchor > length || range.head > length) return null;
  }
  const selection = EditorSelection.create(proposal.ranges.map((r) => EditorSelection.range(r.anchor, r.head)), proposal.mainIndex);
  // CM merges overlapping ranges; refuse ambiguous selection payloads.
  if (selection.ranges.length !== proposal.ranges.length) return null;
  return { changes, selection, scrollIntoView: proposal.scroll === true,
    annotations: [Transaction.userEvent.of("plugin"), isolateHistory.of("full")] };
}

const setPluginSuggestion = StateEffect.define<{ suggestion: PluginSuggestion; selected: number } | null>();
const pluginSuggestions = StateField.define<{ suggestion: PluginSuggestion; selected: number } | null>({
  create: () => null,
  update(value, transaction) {
    if (transaction.docChanged || transaction.selection) value = null;
    for (const effect of transaction.effects) if (effect.is(setPluginSuggestion)) value = effect.value;
    return value;
  },
  provide: field => showTooltip.compute([field], state => {
    const popup = state.field(field);
    if (!popup) return null;
    const { suggestion, selected } = popup;
    return { pos: state.selection.main.head, above: false, strictSide: false, arrow: false,
      create() {
        const dom = document.createElement("div");
        dom.className = "plugin-suggest action-menu";
        dom.setAttribute("role", "listbox");
        suggestion.items.forEach((item, index) => {
          const row = document.createElement("button");
          row.type = "button";
          row.setAttribute("role", "option");
          row.setAttribute("aria-selected", String(index === selected));
          if (index === selected) row.classList.add("selected");
          const label = document.createElement("span");
          label.textContent = item.label;
          row.appendChild(label);
          if (item.secondary) { const secondary = document.createElement("small"); secondary.textContent = item.secondary; row.appendChild(secondary); }
          row.onmousedown = event => event.preventDefault();
          row.onclick = event => pluginHost.chooseSuggestion(suggestion, item.id, { key: "Enter", shiftKey: event.shiftKey,
            ctrlKey: event.ctrlKey, metaKey: event.metaKey, altKey: event.altKey });
          dom.appendChild(row);
        });
        return { dom, mount() { dom.children[selected]?.scrollIntoView({ block: "nearest" }); } };
      },
    } satisfies Tooltip;
  }),
});

/** The Markdown editor: live preview or plain source, with a toolbar. */
export function NoteEditor({ initialText, live, onChange, onEditorUpdate, onOpenLink, complete, cite, ref }: Props) {
  const { t } = useI18n();
  const host = useRef<HTMLDivElement>(null);
  const view = useRef<EditorView | null>(null);
  const preview = useRef(new Compartment());
  const [contextMenu, setContextMenu] = useState<{ x: number; y: number; menus: EditorMenu[] } | null>(null);
  const contextRequest = useRef(0);
  const contextPopup = useRef<HTMLDivElement>(null);
  const closeContextMenu = () => { contextRequest.current++; setContextMenu(null); pluginHost.closeEditorMenus(); };
  const currentMode = useRef(live);
  const callbacks = useRef({ onChange, onEditorUpdate, onOpenLink, complete, cite });
  callbacks.current = { onChange, onEditorUpdate, onOpenLink, complete, cite };
  const identity = useRef({ session: `web-editor-${Date.now()}-${++nextSession}`, generation: 1, revision: 0, contextRevision: 0 });
  const snapshot = (): EditorSnapshot | null => {
    const editor = view.current;
    return editor ? { protocol: 1, ...identity.current, value: editor.state.doc.toString(),
      ranges: editor.state.selection.ranges.map(({ anchor, head }) => ({ anchor, head })), mainIndex: editor.state.selection.mainIndex,
      mode: currentMode.current ? "live" : "source", focused: editor.hasFocus, composing: editor.composing } : null;
  };

  const run = (command: Command) => {
    const editor = view.current;
    if (!editor) return false;
    const { from, to } = editor.state.selection.main;
    const edit = command(editor.state.doc.toString(), from, to);
    editor.dispatch({
      changes: { from: edit.from, to: edit.to, insert: edit.insert },
      selection: { anchor: edit.selectFrom, head: edit.selectTo },
      scrollIntoView: true,
    });
    editor.focus();
    return true;
  };

  useEffect(() => {
    const open = (target: string) => callbacks.current.onOpenLink(target);
    const compositionChanged = (editor: EditorView) => {
      // CM updates its composition flag after DOM observers run.
      queueMicrotask(() => {
        if (view.current !== editor) return;
        identity.current.contextRevision++;
        callbacks.current.onEditorUpdate?.("composition");
      });
    };
    const editor = new EditorView({
      parent: host.current!,
      state: EditorState.create({
        doc: initialText,
        extensions: [
          history(),
          pluginSuggestions,
          pluginEditor,
          Prec.highest(EditorView.domEventHandlers({ keydown(event, editor) {
            const popup = editor.state.field(pluginSuggestions);
            if (!popup || event.isComposing || editor.composing) return false;
            const { suggestion, selected } = popup;
            const modifiers = [event.shiftKey && "Shift", event.ctrlKey && "Ctrl", event.altKey && "Alt", event.metaKey && "Meta"].filter(Boolean).sort().join();
            const custom = suggestion.keys.some(binding => binding.key.toLowerCase() === event.key.toLowerCase() &&
              binding.modifiers.map(m => m === "Mod" ? (/Mac/.test(navigator.platform) ? "Meta" : "Ctrl") : m === "Control" ? "Ctrl" : m === "Command" ? "Meta" : m).sort().join() === modifiers);
            if (!custom && !["ArrowUp", "ArrowDown", "Enter", "Tab", "Escape"].includes(event.key)) return false;
            if (event.key === "ArrowUp" || event.key === "ArrowDown") {
              const index = (selected + (event.key === "ArrowDown" ? 1 : -1) + suggestion.items.length) % suggestion.items.length;
              editor.dispatch({ effects: setPluginSuggestion.of({ suggestion, selected: index }) });
              // Mirror navigation into Scope so plugin selection locks and custom handlers stay in sync.
              pluginHost.chooseSuggestion(suggestion, suggestion.items[selected].id, { key: event.key, shiftKey: event.shiftKey,
                ctrlKey: event.ctrlKey, altKey: event.altKey, metaKey: event.metaKey });
            } else if (event.key === "Escape") {
              pluginHost.chooseSuggestion(suggestion, suggestion.items[selected].id, { key: event.key, shiftKey: event.shiftKey,
                ctrlKey: event.ctrlKey, altKey: event.altKey, metaKey: event.metaKey });
              pluginHost.closeSuggestions();
            }
            else pluginHost.chooseSuggestion(suggestion, suggestion.items[selected].id, { key: event.key, shiftKey: event.shiftKey,
              ctrlKey: event.ctrlKey, altKey: event.altKey, metaKey: event.metaKey });
            event.preventDefault();
            return true;
          } })),
          EditorState.allowMultipleSelections.of(true),
          EditorView.domEventObservers({ compositionstart: (_event, editor) => compositionChanged(editor),
            compositionend: (_event, editor) => compositionChanged(editor) }),
          drawSelection(),
          highlightSelectionMatches(),
          EditorView.lineWrapping,
          EditorView.contentAttributes.of({ "aria-label": t("Note") }),
          markdown({ base: markdownLanguage }),
          syntaxHighlighting(noteHighlight),
          placeholder(t("Start writing…")),
          linkCompletion(
            (query) => callbacks.current.complete(query),
            (query) => callbacks.current.cite?.(query) ?? Promise.resolve([]),
          ),
          preview.current.of(notePreview(live, open)),
          keymap.of([
            { key: "Mod-b", run: () => run(bold) },
            { key: "Mod-i", run: () => run(italic) },
            { key: "Mod-k", run: () => run(wikiLink) },
            ...defaultKeymap,
            ...historyKeymap,
            ...searchKeymap,
            indentWithTab,
          ]),
          EditorView.updateListener.of((update) => {
            if (update.docChanged) identity.current.revision++;
            if (update.docChanged || update.selectionSet || update.focusChanged) identity.current.contextRevision++;
            if (update.docChanged) callbacks.current.onChange(update.state.doc.toString());
            if (update.docChanged || update.selectionSet || update.focusChanged) {
              contextRequest.current++; setContextMenu(null);
              const userEvent = update.transactions.map((tr) => tr.annotation(Transaction.userEvent)).find(Boolean);
              const changes: EditorProposal["edits"] = [];
              update.changes.iterChanges((from, to, _fromB, _toB, insert) => changes.push({ from, to, insert: insert.toString() }));
              callbacks.current.onEditorUpdate?.(userEvent === "plugin" ? "plugin" : "user", userEvent, changes);
            }
          }),
        ],
      }),
    });
    view.current = editor;
    const unsubscribe = pluginHost.subscribeSuggestions(() => {
      const suggestion = pluginHost.suggestionSnapshot();
      // Host publication can occur inside CM's update listener.
      queueMicrotask(() => {
        if (view.current !== editor || pluginHost.suggestionSnapshot() !== suggestion) return;
        const current = snapshot();
        const valid = suggestion && current && suggestion.session === current.session && suggestion.generation === current.generation &&
          suggestion.revision === current.revision && suggestion.contextRevision === current.contextRevision;
        editor.dispatch({ effects: setPluginSuggestion.of(valid ? { suggestion, selected: suggestion.selected } : null) });
      });
    });
    const unsubscribeDecorations = subscribePluginDecorations(() => queueMicrotask(() => {
      const current = snapshot();
      if (view.current !== editor || !current) return;
      const results = currentPluginDecorations().filter(r => r.session === current.session && r.generation === current.generation &&
        r.revision === current.revision && r.contextRevision === current.contextRevision);
      editor.dispatch({ effects: setPluginDecorations.of({ results, live: currentMode.current }) });
    }));
    return () => { unsubscribeDecorations(); unsubscribe(); pluginHost.closeSuggestions(); pluginHost.closeEditorMenus(); view.current = null; identity.current.generation++; editor.destroy(); };
    // The editor is made once per note (the parent keys it by path).
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    const open = (target: string) => callbacks.current.onOpenLink(target);
    view.current?.dispatch({ effects: [setPluginDecorations.of({ results: [], live }), preview.current.reconfigure(notePreview(live, open))] });
    if (currentMode.current !== live) {
      currentMode.current = live;
      identity.current.contextRevision++;
      callbacks.current.onEditorUpdate?.("context");
    }
  }, [live]);

  useEffect(() => {
    if (!contextMenu) return;

    const outside = (event: Event) => {
      if (!contextPopup.current?.contains(event.target as Node)) closeContextMenu();
    };
    let selected = 0;
    const navigate = (event: KeyboardEvent) => {
      if (event.isComposing) return;
      const buttons = [...(contextPopup.current?.querySelectorAll<HTMLButtonElement>("button:not(:disabled)") ?? [])];
      if (!["ArrowUp", "ArrowDown", "Home", "End", "Enter", "Escape", "Tab"].includes(event.key)) return;
      event.preventDefault(); event.stopImmediatePropagation();
      if (event.key === "Escape" || event.key === "Tab") closeContextMenu();
      else if (event.key === "Enter") buttons[selected]?.click();
      else {
        selected = event.key === "Home" ? 0 : event.key === "End" ? buttons.length - 1 :
          (selected + (event.key === "ArrowDown" ? 1 : -1) + buttons.length) % buttons.length;
        buttons.forEach((button, index) => button.classList.toggle("selected", selected === index));
        buttons[selected]?.scrollIntoView({ block: "nearest" });
      }
    };
    contextPopup.current?.querySelector("button:not(:disabled)")?.classList.add("selected");
    document.addEventListener("keydown", navigate, true);
    document.addEventListener("pointerdown", outside);
    document.addEventListener("scroll", outside, true);
    window.addEventListener("resize", outside);
    return () => { document.removeEventListener("keydown", navigate, true); document.removeEventListener("pointerdown", outside); document.removeEventListener("scroll", outside, true); window.removeEventListener("resize", outside); };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [contextMenu]);

  useImperativeHandle(ref, () => ({
    goToLine(line: number) {
      const editor = view.current;
      if (!editor) return;
      const target = editor.state.doc.line(Math.min(line + 1, editor.state.doc.lines));
      editor.dispatch({ selection: { anchor: target.from }, scrollIntoView: true });
      editor.focus();
    },
    insert(text: string) {
      const editor = view.current;
      if (!editor) return;
      const { from, to } = editor.state.selection.main;
      editor.dispatch({ changes: { from, to, insert: text }, selection: { anchor: from + text.length }, scrollIntoView: true });
      editor.focus();
    },
    state() {
      return snapshot();
    },
    apply(proposal) {
      const editor = view.current;
      const current = snapshot();
      if (!editor || !current) return false;
      const transaction = pluginTransaction(editor.state, current, proposal);
      if (!transaction) return false;
      editor.dispatch(transaction);
      if (proposal.focus) editor.focus();
      return true;
    },
    setValue(text: string) {
      const editor = view.current;
      if (!editor) return;
      const anchor = Math.min(editor.state.selection.main.anchor, text.length);
      editor.dispatch({ changes: { from: 0, to: editor.state.doc.length, insert: text }, selection: { anchor } });
    },
  }));

  const tool = (label: string, command: Command, content: string, className?: string) => (
    <button
      type="button"
      className={className}
      title={t(label)}
      aria-label={t(label)}
      onMouseDown={(e) => e.preventDefault()}
      onClick={() => run(command)}
    >
      {content}
    </button>
  );

  return (
    <div className="note-editor">
      <div className="note-toolbar" role="toolbar" aria-label={t("Formatting")}>
        {tool("Bold", bold, "B", "bold")}
        {tool("Italic", italic, "I", "italic")}
        {tool("Underline", (d, f, e) => toggleWrap(d, f, e, "<u>", "</u>"), "U", "underline")}
        {tool("Strikethrough", (d, f, e) => toggleWrap(d, f, e, "~~"), "S", "strike")}
        <span className="divider" />
        {tool("Heading 1", (d, f, e) => toggleLinePrefix(d, f, e, "# "), "H1")}
        {tool("Heading 2", (d, f, e) => toggleLinePrefix(d, f, e, "## "), "H2")}
        {tool("Heading 3", (d, f, e) => toggleLinePrefix(d, f, e, "### "), "H3")}
        <span className="divider" />
        {tool("Bulleted List", (d, f, e) => toggleLinePrefix(d, f, e, "- "), "•")}
        {tool("Numbered List", (d, f, e) => toggleLinePrefix(d, f, e, "1. "), "1.")}
        {tool("Task", (d, f, e) => toggleLinePrefix(d, f, e, "- [ ] "), "☐")}
        {tool("Quote", (d, f, e) => toggleLinePrefix(d, f, e, "> "), "❝")}
        {tool("Code", (d, f, e) => toggleWrap(d, f, e, "`"), "</>")}
        {tool("Link to Note", wikiLink, "[[ ]]")}
        {tool("Table", (_d, f, e) => insertText(f, e, TABLE, 2), "▦")}
        <span className="divider" />
        <MenuButton
          label={t("Text Color")}
          icon={<span className="text-swatch" aria-hidden="true">A</span>}
          iconOnly
          items={TEXT_COLORS.map((color) => ({
            label: `${t("Text Color")} ${color}`,
            icon: <span className="menu-swatch text-swatch" style={{ color }}>A</span>,
            onSelect: () => run((d, f, e) => colorText(d, f, e, color, false)),
          }))}
        />
        <MenuButton
          label={t("Highlight Color")}
          icon={<span aria-hidden="true">▰</span>}
          iconOnly
          items={BACKGROUND_COLORS.map((color) => ({
            label: `${t("Highlight Color")} ${color}`,
            icon: <span className="menu-swatch" style={{ background: color }} />,
            onSelect: () => run((d, f, e) => colorText(d, f, e, color, true)),
          }))}
        />
      </div>
      <div className={live ? "cm-host live" : "cm-host source"} ref={host} onContextMenu={async event => {
        if (!pluginHost.hasEditorMenu()) return;
        event.preventDefault();
        const request = ++contextRequest.current;
        const x = event.clientX, y = event.clientY;
        const menus = await pluginHost.editorMenu();
        if (request === contextRequest.current && menus.length) setContextMenu({ x, y, menus });
      }} />
      {contextMenu && createPortal(<div ref={contextPopup} className="action-menu plugin-editor-menu" role="menu"
        style={{ top: Math.max(8, Math.min(contextMenu.y, window.innerHeight - 300)), left: Math.max(8, Math.min(contextMenu.x, window.innerWidth - 300)) }}
        onBlur={event => { if (!event.currentTarget.contains(event.relatedTarget)) closeContextMenu(); }}
        >
        {contextMenu.menus.flatMap(menu => menu.items.map((item, index) => "separator" in item ? <hr key={`${menu.plugin}-${index}`} /> :
          "children" in item ? <div key={`${menu.plugin}-${index}`}><span className="plugin-menu-group">{item.label}</span>
            {item.children.map((child, childIndex) => "separator" in child ? <hr key={childIndex} /> :
              <button key={childIndex} role="menuitem" disabled={item.disabled || child.disabled} onMouseDown={event => event.preventDefault()}
                onClick={event => { pluginHost.chooseEditorMenu(menu, child.index, event); closeContextMenu(); }}>{child.label}</button>)}</div> :
          <button key={`${menu.plugin}-${index}`} role="menuitem" disabled={item.disabled} onMouseDown={event => event.preventDefault()}
            onClick={event => { pluginHost.chooseEditorMenu(menu, item.index, event); closeContextMenu(); }}>{item.label}</button>))}
      </div>, document.body)}
    </div>
  );
}
