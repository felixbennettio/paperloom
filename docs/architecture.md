# Architecture

Paperloom is one product on five platforms with two user interfaces and one
shared core.

```
                 core/ (Rust)
   model · store (SQLite) · vault (Markdown) · fulltext (Tantivy)
   metadata · search · citation · formats · account · paperloom-core API
        │                          │
   UniFFI → Swift              Rust crate
        │                          │
  apple/ (SwiftUI)           app/ (Tauri 2 + React)
  macOS · iOS · iPadOS       Windows · Linux · Android
```

Zotero web translators are optional files in `<library folder>/translators/`.
Settings downloads their top-level JavaScript files from GitHub only on request;
the AGPL translators are kept separate from the app and are never bundled.
The Windows, Linux, macOS, iPadOS and iOS apps run Paperloom's independently
implemented translation environment in a hidden, sandboxed frame served by the
`plugin:` scheme. The Apple apps reuse the plugin host's WebKit scheme handler,
wrapper and message relay with a separate, nonpersistent web view. It reads a
bounded HTML snapshot of the person's browser page, asks the host for translator
code, network requests and item selection, and returns completed items through the
existing search-result and Add path. Translation has a 60-second limit; network
requests have a 30-second limit. The Apple host checks for unanswered messages
after five seconds, allowing core requests and the item checklist to finish
through their own completion paths.

## Rules

- **Behaviour lives in the core.** Parsing, storage, search, sync, citation
  formatting and AI calls are written once in Rust. The UIs show data and
  forward actions; if both UIs would need the same logic, it belongs in the core.
- **The core's API is small and made of plain records** (`ItemSummary`,
  `NewItem`, `CollectionInfo`) so it maps cleanly to Swift and to JSON for
  the Tauri commands.
- **Licences:** only permissive dependencies (MIT, Apache-2.0, BSD, Zlib,
  ISC, MPL-2.0 unmodified). No GPL, AGPL or statically linked LGPL code.
- **Translations** are written once in `i18n/strings.json` and generated into
  both apps by `scripts/i18n.py`; CI fails when a language is incomplete.
  Launch languages: Simplified Chinese, Traditional Chinese, English, French,
  Japanese, Russian.

## Crates

| Crate | Purpose |
| --- | --- |
| `paperloom-model` | Items (CSL-like), creators (CJK names kept whole), DOIs, collections, IDs (UUID v7) |
| `paperloom-store` | One SQLite file per library; items stored as JSON beside indexed columns; migrations via `user_version`; duplicate items found (the same DOI, book ISBN, arXiv or PubMed ID, or the same title with close years and a shared author) and merged; saved searches (named conditions, run afresh each time); related items (a row each way, synced as the items' `related:` fields); feeds followed and their entries, kept on the device |
| `paperloom-http` | The HTTP client behind every network feature (ureq + rustls), as a trait so tests answer calls themselves |
| `paperloom-account` | Paperloom account sign-in: OIDC authorization code + PKCE against Logto |
| `paperloom-metadata` | Details from a DOI (doi.org CSL-JSON), arXiv ID, PubMed ID or ISBN |
| `paperloom-search` | Feeds (RSS 2.0, RSS 1.0 with Dublin Core and PRISM, Atom) read into works; online search: the open databases (Crossref, OpenAlex, arXiv, PubMed, Semantic Scholar), library catalogues over SRU (Library of Congress, K10plus, and any the person adds, which the apps keep on the device) or over Z39.50 itself (its BER messages, ISO 2709 records in MARC 21 or UNIMARC / CNMARC, in UTF-8, MARC-8 or a named character set such as GBK) and, with the person's own key, Web of Science, Scopus, ScienceDirect, IEEE Xplore and Springer Nature |
| `paperloom-capture` | Reads works from web pages in the in-app browser: embedded metadata (Highwire `citation_*`, Dublin Core, PRISM, COinS, JSON-LD) and, for CNKI, Wanfang and VIP, site rules kept as data in `core/rules/sites.json` |
| `paperloom-citation` | Citations and reference lists in CSL styles (hayagriva), GB/T 7714 first; styles added from the CSL style repository or `.csl` files |
| `paperloom-formats` | Reference files: RIS, EndNote tagged (CNKI), BibTeX and CSL-JSON in and out; PubMed MEDLINE, Web of Science, Scopus CSV and EndNote XML in |
| `paperloom-vault` | The Markdown vault: plain `.md` files (Obsidian-compatible), links, backlinks, tags, renames that rewrite links, rendering to HTML; properties written back as front matter, templates and daily notes (settings shared with Obsidian in `.obsidian/`), earlier versions of each note (in `.paperloom/history/`), and the graph of links, laid out; `query` blocks that list notes or library items by tag, folder or collection and property; canvases (JSON Canvas); mind maps (outlines with a `mindmap` property, laid out here, to and from OPML, FreeMind and XMind's `content.json`/`content.xml`, and as SVG; coloured by one of six themes named in a `mindmap-theme` property; topics carry markers, links and labels); drawings (Excalidraw scenes with their pictures, drawn as SVG); diagrams in draw.io's format (`.drawio.svg`, `.drawio`): read and written keeping what Paperloom does not use, laid out (shapes, containers, groups, layers), edges routed straight, orthogonally or curved, pictures (kept in the style as draw.io keeps them), edited, and drawn as SVG |
| `paperloom-fulltext` | Full-text search over items, PDF text, annotations and notes (Tantivy; jieba for Chinese) |
| `paperloom-sync-proto` | The sync protocol's wire types, shared by the apps and the sync service |
| `paperloom-sync` | The sync engine: the library's records in a Loro document, merged field by field and exchanged as a change log |
| `paperloom-core` | The API both apps call; UniFFI exports for Swift. Also: finding an item's open-access PDF (its arXiv copy, the open copies OpenAlex lists, the PDF its publisher's page names) and attaching it, moving a Zotero library (its database) or an EndNote library (its XML export and `.Data` folder) in, Word and LaTeX export, a Word, WPS or LibreOffice document's (`.docx` or `.odt`) `[@citations]` formatted in a copy of it (with the reference list where `{bibliography}` stands, or at the end), backups, sync and its end-to-end encryption, XMind files (ZIPs) in and out, diagrams for the apps' own editors (a page laid out, changes, the shape palette), and PDF forms (an AcroForm's fields read with where their boxes lie, and values written back with appearances other readers show) |
| `uniffi-bindgen` | Generates the Swift bindings (`scripts/build-apple-core.sh`) |

## Data on disk

- `library.sqlite`: items, collections, tags, attachments, annotations (in
  PDF page coordinates, so every app draws the same marks), saved searches
  (their conditions as JSON) and PDF page text. It is the source of truth.
- `files/<attachment id>/<file name>`: attached files.
- `index/`: the full-text index, derived from the library (and the vault);
  rebuilt when its format changes.
- `styles/<id>.csl`: citation styles added from the CSL style repository
  (Zotero's) or a file, added again when the library opens, and
  `styles/index.json`, the repository's list of styles, downloaded again
  after a week. They stay on the device (sync leaves them out).
- A backup (`PaperLibrary::back_up`) is one `.zip`: `paperloom-backup.json`,
  `library.sqlite` (a `VACUUM INTO` copy), `files/…`, `styles/…` and `vault/…`, stored
  uncompressed (ZIP64 when big). Putting one back unpacks the library to
  `restoring/` beside this one; `finish_restore`, called before the library
  opens, swaps it in and moves what was there to `before-restore-<time>/`.
- `sync/`: for a synced library, its Loro document and the sync state
  (cursor, outbox). The library notes which records changed in
  `sync_changed` (filled by triggers).
- The vault: a folder of `.md` files, by default `Vault/` beside the library;
  any folder (an Obsidian vault) can be opened instead. Inside it,
  `.obsidian/daily-notes.json` and `.obsidian/templates.json` hold the daily
  note and template settings (Obsidian reads the same files), and
  `.paperloom/history/<hash of the path>/<time>.md` the earlier versions of
  each note: before a save changes a note, it is kept as it was, at most
  every five minutes, the last 100.
- In the vault beside the notes, other apps' formats, so they open the same
  files: canvases as JSON Canvas (`.canvas`), drawings as Excalidraw scenes
  (`.excalidraw`; elements and fields Paperloom does not draw are kept, and
  a stroke is `strokeWidth × 4.25 × (0.4 + 1.2 × pressure)` wide, as
  Excalidraw draws it), and diagrams as `.drawio.svg` (a picture with the
  diagram in its `content` attribute) or `.drawio`. A mind map is a note:
  its outline is the topics, and its relations, summaries and boundaries
  sit in a closing `%% mindmap … %%` comment (hidden in Obsidian's reading
  view), naming topics by block ID (`- Topic ^t1`; the centre is
  `centre`). Markers are the emoji at the start of a topic's text, and a
  topic's link is Markdown around its text (`[Topic](https://…)` or
  `[[Note|Topic]]`), so links in maps are links like any other; its
  labels are the tags at the end of its text (`Topic #draft`).

## Sync service

`server/` is Paperloom Cloud, the official sync service (axum, PostgreSQL,
S3-compatible storage through OpenDAL). It keeps each library's change log
and its blobs without reading either, so they can be end-to-end encrypted.
Code and tests only for now; it is not deployed. The protocol, settings and
deployment are in [sync-protocol.md](sync-protocol.md).

Libraries can also sync through storage people bring: a shared folder,
WebDAV, an S3 bucket, or a Seafile library through its
[Web API v2](https://seafile-api.readme.io/). The core's `Shelf` transport
writes device change logs and blobs through the same HTTP client as its
other network features. Seafile finds the library by name or creates it,
makes missing folders, and uses upload and download links to transfer files.
Its folder setting is a path inside the library, empty for the top.
The apps keep its password separately in their secret store or keychain,
just like WebDAV. Exactly 40 ASCII hexadecimal characters are treated as
an account API token; all other credentials are exchanged for a token by
signing in. Account tokens are sent only to the API server, and transport
errors omit credentials, response bodies and signed file URLs.

## Apple app

`apple/project.yml` (XcodeGen) defines `Paperloom-iOS` and `Paperloom-macOS`.
`apple/CoreKit` is a Swift package wrapping the Rust XCFramework and the
generated bindings, both produced by `scripts/build-apple-core.sh`. The UI
uses Liquid Glass (`glassEffect`, `GlassEffectContainer`, `.buttonStyle(.glass)`),
so the deployment target is iOS/macOS 26.

The Mac app ships twice from the same target: the App Store build (App
Sandbox) and the direct download (Developer ID, notarized, sandbox
entitlements swapped out in `build.yml`).

## Tauri app

`app/` is a React + TypeScript UI (`src/`) and the Tauri shell
(`src-tauri/`), whose commands wrap `paperloom-core`. In a plain browser the
UI falls back to an in-memory backend (`src/lib/api.ts`) for development.

PDF text comes from each app's own PDF engine (PDFKit on Apple, pdf.js in the
web UI) and is kept by the core for search and the AI. A page with almost no
text is a scanned picture: Apple reads it with Vision, on the device; the
Tauri app draws it with pdf.js and hands the PNG to `src-tauri/src/ocr.rs`,
which uses Windows' own text recognition (`Windows.Media.Ocr`), on Linux
Tesseract where it is installed, and on Android ML Kit's Chinese model (kept
in the app; it reads Latin letters too) through the share plugin.

## Plugins

Obsidian community plugins run in both apps from the vault's own
`.obsidian/plugins/<id>/`, the ones listed in
`.obsidian/community-plugins.json` turned on (Obsidian reads the same
files). The runtime, `core/plugin-runtime/` (plain JavaScript, shared by
both apps), gives a plugin the `obsidian` module. Each plugin runs in its
own frame, served from the app's `plugin:` scheme with
`sandbox="allow-scripts"`: an opaque origin with no access to the app's
pages, its IPC or its storage. Everything a plugin does goes to the host as
a message: the vault, the open note's editor, its settings (`data.json`)
and web requests. The host is `app/src/lib/plugins.ts` in the Tauri app and
`apple/Paperloom/Plugins/` on Apple, and a plugin reaches only the sites the
person allowed for it. Its settings tab and modals show as its frame over
the app. Its commands run from the command palette (⌘P / Ctrl+P). Code
blocks it registers are drawn in its frame and shown as cleaned HTML, once
what it draws stops changing (renderers that wait for an index draw twice).
Since the frame has no storage, the runtime gives it `localStorage` and an
IndexedDB in memory (localForage and Dexie caches work, rebuilt each start),
Obsidian's DOM helpers, and stand-ins for the CodeMirror packages and global
Obsidian hands plugins, so plugins with editor extensions (Dataview) load
and do everything else.
Community plugins install from Obsidian's community list and the plugin's
latest GitHub release, as Obsidian installs them. Plugins that need DOM
outside their frame, editor extensions or views are marked as working
partly.

Zotero 7 bootstrap plugins use the shared `zotero.js` compatibility layer
in the same sandbox, including item pane sections and item list columns;
column values are requested in batches and shown as plain text in the web
table and as labelled secondary lines in Apple item lists.
Reader extension points provide selection popups, annotation headers, toolbar
fragments and context menu commands through host messages, with sanitized HTML
on the web and plain text on Apple.

## Browser extension

`extension/` is a Manifest V3 extension (Chrome, Edge, Firefox). Its popup
runs the same capture script as the in-app browser (`capture.js`, from
`core/crates/capture/src/extract.js` and `core/rules/sites.json`) in the
page, and sends the answer to the desktop app's connector: a small HTTP
server on `127.0.0.1:23121` (`PaperLibrary::start_connector`) that reads
the page (`POST /read`) and adds the works picked (`POST /save`).
Requests from web pages (an `http…` origin) are refused.
`scripts/build-extension.py` puts it together in `target/extension` (load
it unpacked from there) and zips it; CI keeps the zip.

## CI

Builds run on the free tiers of several services (docs/ci.md). Only the
checks run by themselves (CircleCI, every push); installable builds are
started by hand or by a `v*` tag, never on a schedule: Google Cloud Build
makes the Linux and Android apps, Azure Pipelines the Windows app, Xcode
Cloud the Apple apps (TestFlight, the notarized DMG). Each has a fallback
on another service for when its free minutes run out. Every non-Apple job is one `scripts/ci/run.sh <job>`, the same on
every service; on a `v*` tag each service adds its own files to the GitHub
release (`scripts/ci/github_release.py`).

The self-hosted GitLab (git.routeforge.zone) mirrors this repository and
runs GitLab CI (`.gitlab-ci.yml`):

| Pipeline | When | What |
| --- | --- | --- |
| checks | every push and merge request | `scripts/ci/run.sh core`, `server` (with a throwaway PostgreSQL) and `ui` on Linux, then the Apple build and CoreKit tests |
| nightly | schedule at 03:00 Beijing when main moved, or Run pipeline with `BUILD=true` | `.gitlab/ci/build.yml`: every app to try out, as artifacts; iOS and macOS to TestFlight with `TESTFLIGHT=true` |
| release | `v*` tag | `.gitlab/ci/build.yml` with TestFlight, then a GitLab release with checksums |

The Linux, macOS and Windows runners are separate VMs on one machine, each
taking one job at a time; ubuntu Rust jobs run at most six compilers in
their 7 GB container.

Versions: `scripts/check_versions.py --set X.Y.Z` updates every manifest;
the release tag must match.
