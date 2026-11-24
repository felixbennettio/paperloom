# Writing plugins (Word, WPS)

Citing while writing: a task pane in Word (an Office.js add-in) or WPS (a
JS add-in) searches the library, inserts citations, keeps their numbers and
the reference list up to date and changes the style. The plan and its
decisions: `~/.claude/plans/writing-plugin-plan.md`. Step 1, formatting
`[@key]` markers in a copy of a document, is `core/crates/core/src/wordcite.rs`.

The add-ins talk to the Paperloom app on the same computer through the
connector (`core/crates/core/src/connector.rs`, `http://127.0.0.1:23121`),
which the browser extension uses too. The task pane's code is shared
(`app/src/addin/`); each word processor has a document adapter.

## Connector: the cite API

All bodies are JSON (UTF-8). Every `/cite/...` request except `/cite/pair`
carries `Authorization: Bearer <token>`; without a known token the answer is
`401 {"error": …}`. Requests may come from web pages (the add-ins are web
pages); the connector answers CORS preflights (`OPTIONS`) for them, with
`Access-Control-Allow-Private-Network: true` when asked. Errors are
`{"error": "<message for people>"}`.

- `GET /ping` → `{"app": "Paperloom", "version": "0.1.0", "cite": 2}`.
  `cite` is the version of this API (2 added `/cite/markers`).
- `POST /cite/pair` `{"client": "Word"}` → `200 {"token": "<64 hex>"}` once
  the person allows it in the app (which names the client and the page's
  origin), `403 {"error"}` when they decline, `409 {"error"}` while another
  request waits for an answer. Tokens are kept by the app
  (`connector-clients.json` beside the library) until removed there.
- `GET /cite/styles` → `[{"id": "gb-7714-2015-numeric", "name": "GB/T 7714—2015 (顺序编码制)"}, …]`,
  the built-in ones first, then those the person added.
- `POST /cite/search` `{"query": "kucsko 2013", "limit": 20}` →
  `[{"id": "<item id>", "key": "kucsko2013nanometre", "title": "…",
  "cite": "Kucsko et al. 2013", "year": 2013}]`: items whose title, authors,
  year or citation key hold every word, newest first. An empty query lists
  the newest.
- `POST /cite/format` `{"styleId": "…", "clusters": [{"works":
  [{"itemId": "…", "locator": "12"}]}], "html": true}` →
  `{"citations": ["[1]", …], "bibliography": ["[1] KUCSKO G, …", …],
  "missing": ["<item id>", …]}`. `clusters` are the document's citations in
  document order, each the works cited at one place (`locator`, optional, is
  a page). `citations` has one entry per cluster; `bibliography` one per
  reference list entry, numbered across the document. With `html`, the text
  keeps italics, bold and super/subscript as HTML (`<i>`, `<b>`, `<sup>`,
  `<sub>`). Works no longer in the library are left out and listed in
  `missing`; a cluster with none left formats as `""`.
- `POST /cite/markers` `{"text": "<the document's text>"}` →
  `[{"text": "[@kucsko2013, p. 3]", "works": [{"itemId": "…", "locator":
  "3"}], "missing": []}, …]`: the Pandoc citations in the text, in order,
  read as Cite in Word Document reads them (keys are citation keys, DOIs or
  item IDs). `works` is empty when the library lacks one of the keys, which
  `missing` lists. The task pane's "Convert Citation Markers" makes each
  place a marker is written a citation of its works (Word: a body search for
  the marker's text, so not markers over 255 characters or with `^`; WPS:
  the marker's place in `Content.Text`, looked for past the fields' marks)
  and leaves the others as written.

## In the document

Each citation is a field the add-in can find again (a content control in
Word, the equivalent in WPS) with an id; the works it cites, the style and
a snapshot of each cited work's citation text live in the document (Word:
the add-in's document settings; WPS: document variables), so a document
opened without Paperloom, or on another computer, keeps its text. "Convert
to plain text" removes the fields and keeps the text.

## Word verification version

`app/src/addin/officeDocument.ts` implements the shared `DocumentAdapter`.
Each rich-text control has tag `paperloom:cite:<uuid>` and title
`Paperloom citation`; the single reference list has tag
`paperloom:bibliography`. Reads load `document.contentControls` and retain
the collection's document order, rather than sorting ids or using the
settings' insertion order. Controls removed in Word have their metadata
pruned on the next adapter operation. A control with missing metadata
reports an error instead of inventing works or overwriting its text.

The document setting `paperloom` holds one JSON object:
`{version: 1, citations: [{id, works, text}], bibliography, styleId}`.
`text` and bibliography entries retain the connector's last HTML; an absent
bibliography is `null`, an existing empty one is `[]`. Writes sync Word
first, then await `Office.context.document.settings.saveAsync`. Saving the
`.docx` in Word is still necessary to save the document to disk. Reopening
with the same add-in id restores these settings; no connector token is
stored in the document. The Word token uses local storage key
`paperloom.addin.word.token`.

Only pages with the `_host_Info` query parameter load Office.js, dynamically
from Microsoft's CDN. Initialization waits for `Office.onReady` and checks
for Word and WordApi 1.1. The pane uses Office's display language. Browser
preview and the `window.wps` branch do not load Office.js. A failed
Office startup is shown as an error rather than opening a memory preview.

The adapter uses WordApi 1.1 for `Word.run`/`context.sync`,
`document.contentControls`, collection `load`/`items`,
`document.getSelection`, `Range.insertContentControl()` (rich text by
default), content-control `tag`/`title`, `ContentControl.insertHtml` with
`Replace`, and `ContentControl.delete`. `delete(false)` removes a citation
and its text; `delete(true)` unlinks it while retaining its actual Word
content, including any manual edits and formatting. Unlink also removes
the document setting. Common Office APIs provide `Office.onReady`,
`context.displayLanguage`, requirement checks, and document settings
`get`/`set`/`remove`/`saveAsync` (Settings 1.1).

A new reference list goes in a new paragraph after the cursor's
(`Range.insertParagraph("", "After")`, WordApi 1.1), so it never runs into
the text around it.

One optional call needs WordApi 1.3: after inserting a control,
`ContentControl.getRange("After").select()` moves the caret outside it to
avoid nesting the next insertion. It is gated by `isSetSupported`.
On a 1.1-only host, place the cursor outside the previous control before
inserting another citation. No 1.5 content-control type argument is used.

The add-in-only manifest template is `integrations/word/manifest.xml`.
Materialize it after choosing the HTTPS hosting origin:

```sh
python3 scripts/word-manifest.py /path/to/manifest.xml --origin https://example.pages.dev --version 0.1.0
```

Without `--origin` the script takes `PAPERLOOM_WORD_ADDIN_ORIGIN`, or the
address in `integrations/word/origin.txt` (filled in once the add-in is
deployed), and writes nothing when neither has one. `--version` makes the
manifest's version the app's, as Office keeps an add-in's files by it.

The origin must have no path, query, fragment, credentials or trailing
slash. The script uses only Python's standard library, replaces
`{{ORIGIN}}`, adds manifest locale overrides from `i18n/strings.json`, and
parses the generated XML. The fixed GUID must be retained when updating
or moving the hosted add-in, since document settings belong to that add-in
identity. The add-in build copies `app/addin-public/icons` to
`app/dist-addin/icons`. The Home tab's Paperloom → Cite command opens
`addin.html` on Mac and Windows; other hosts use the same task pane code.
The manifest includes a base task-pane entry for hosts without ribbon
commands.

Settings › Writing Plugins installs the add-in where the app carries a
manifest, that is once `origin.txt` has the hosting address (`run.sh`
`word_manifest` for the Windows package, `ci_post_clone.sh` for the Mac):

- Windows (`app/src-tauri/src/writers.rs`): the manifest is copied to
  `word-addin/manifest.xml` in the app's data folder and listed among
  Word's developer add-ins, as Office's sideloading tools do: value
  `<manifest id>` = `<path>` under
  `HKCU\Software\Microsoft\Office\16.0\WEF\Developer`. Remove deletes both.
  The Windows end-to-end test installs and removes it with a stand-in
  address (`https://word-addin.paperloom.test`).
- Mac (`apple/Paperloom/Settings/WritingPluginsSection.swift`): the
  manifest goes to `Paperloom.xml` in
  `~/Library/Containers/com.microsoft.Word/Data/Documents/wef`, which Word
  for Mac reads when it starts. The Developer ID app writes there itself;
  the App Store app, in its sandbox, asks the person to choose that folder
  (or Word's Documents folder) and keeps a bookmark to it. The same
  section lists the connected writing plugins.

Word shows the Paperloom group on its Home tab after a restart.

Before relying on this in Word for Mac, verify insertion at a collapsed
cursor and over selected text; repeated insertion without nested controls;
moving and deleting citations; refresh, reference-list replacement and
style changes; save, close and reopen; and unlink with visible text
preserved. The fake API tests exercise batching and metadata persistence,
but cannot establish Word's HTML rendering or collection ordering across
different document stories (headers, footers, text boxes). Copied controls
with duplicate tags and cross-document pasting without their settings are
outside this verification version. Word edits and settings saves are two
API operations, so a host or settings failure can leave a partial edit;
it is surfaced to the user and is not an atomic transaction. The HTTPS
task pane's connection to `http://127.0.0.1:23121` still needs verification
in the owner's Mac Word webview. On web/iPad, loopback addresses that
browser/device, which must itself have a reachable Paperloom connector.

## WPS verification version

`app/src/addin/wpsDocument.ts` implements the unchanged `DocumentAdapter`
over `wps.WpsApplication()`. The Paperloom ribbon tab's Cite button opens
the shared pane, docked right; later clicks toggle the same pane. The host
uses the navigator's display language and local storage token key
`paperloom.addin.wps.token`. No token is stored in the document.

Citations and the single bibliography use rich-text content controls with
the same tags and titles as Word. Controls are sorted by `Range.Start`,
independently of collection order. The document variable `paperloom` holds
a JSON string with the same version 1 shape as Word's settings. Operations
are serialized, prune metadata for deleted controls, and replace the
variable's value after changes. Invalid metadata and controls with missing
citation metadata report the same errors as Word. Save the document in WPS
to persist its text and variables to disk.

The adapter uses synchronous `ActiveDocument`, `Selection.Range`,
`ContentControls.Add(0, range)`, `Count`/1-based `Item`, control `Tag`,
`Title`, `Range.Text`/`Start`/`End`, `Document.Range(start, end)` and font
`Italic`/`Bold`/`Superscript`/`Subscript`. HTML passes through `citationHtml`,
then becomes text runs with those four flags; no HTML paste API is needed.
Bibliography entries are joined with line breaks (`\v`) inside one control:
WPS leaves a field empty when its text holds a paragraph mark. A new
reference list stands in a paragraph of its own: a paragraph starts before
it unless the cursor is at a paragraph's start (a field's end mark before
the cursor reads as empty text and counts as inside a paragraph), and after
it unless a paragraph ends there (`Selection.TypeParagraph`). The cursor is
put after a new field at its Range's `End + 1`: `End` is still inside the
control (its end mark takes one more position). Variables use `Item`/`Add`/`Value`/`Delete`. Removing a citation
calls `ContentControl.Delete(true)`; unlink calls `Delete(false)` in reverse
document order and deletes only the Paperloom variable, retaining actual
visible text and formatting, including manual edits.

From `app/`, `npm run build:wps` produces this self-contained folder:

```text
dist-wps/
  ribbon.xml
  index.html       # Hidden ribbon host, loads main.js
  main.js          # Global OnAddinLoad and OnAction callbacks
  pane.html        # Loads pane.css and a classic pane.js script
  pane.js          # One IIFE, including React and the shared task pane
  pane.css
  icons/           # Copied from app/addin-public/icons
```

`vite.wps.config.ts` targets Chrome 104, bundles dependencies locally and
disables code splitting. HTML has no module scripts or CDN dependencies.
The CI ui job checks this build beside the Word task-pane build. The Linux and
Windows packages carry the folder as the `wps-addin` resource, and Settings
› Writing Plugins installs it (`core/crates/core/src/wpsaddin.rs`): it is
copied to `jsaddons/paperloom_<version>/` in WPS's add-in folder
(`~/.local/share/Kingsoft/wps/jsaddons` on Linux, `%APPDATA%\kingsoft\wps\jsaddons`
on Windows) and listed in `jsaddons/publish.xml` as `<jsplugin
name="paperloom" type="wps" version="<version>" enable="enable"
install="null" url="…"/>`. WPS fetches `url` only when the folder is
missing, and rewrites the list itself, so other add-ins' entries are kept as
they are. The first time, WPS asks whether to allow the add-in and whether
to trust it. The same card lists the connected writing plugins and takes
their access back.

The Linux WPS 12.1.2.29183 probe established document controls, partial-range
formatting, document variables saved in DOCX, task panes and file-page
access to loopback HTTP. The packaged add-in still needs a real WPS run:
selection replacement; repeated insertion without nesting; moving and
deleting controls; numbering, style changes and bibliography paragraphs;
save/close/reopen; connector pairing/search; and unlink preserving manual
formatting. Caret movement first tries `Selection.SetRange(end, end)`, then
`Selection.Range.SetRange` plus `Range.Select`; both paths are guarded but
still need host verification. Also verify the deletion flags,
`Variable.Delete`, missing-variable lookup (a thrown lookup is treated as
absence), UTF-16 range offsets, bold/subscript and clearing old font flags,
and `Range.Text` with paragraph marks. A
failure is surfaced without a destructive fallback. Document edits and
metadata writes are separate operations and can leave a partial edit on
failure. Fake API tests establish adapter behavior, not WPS rendering or
disk durability. Duplicate tags and cross-document pasting without metadata
remain outside this version.
