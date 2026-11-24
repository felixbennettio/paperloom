# CI and builds

The code lives on GitHub (private), at `routeforge-dev/paperloom`. Builds run on the free tiers of several
services, each doing what its free tier suits best. The steps of every
non-Apple job are in `scripts/ci/run.sh`, so the services' configs only pick
a machine, pass secrets and keep the files; changing how something is built
means changing `run.sh` once.

Only the checks run by themselves. Every installable build is started by
hand, or by pushing a `v*` tag for a release; nothing runs on a schedule.

| Job | Architectures | Runs on | Fallback when out of minutes |
| --- | --- | --- | --- |
| Checks: core, sync service, UI, Tauri clippy | Linux x64 | CircleCI, every push | Azure, Run pipeline with `target: checks` (`checks` names which of `core server ui`) |
| Linux app (AppImage, deb, rpm) and Android APK | Linux x64, Android arm64 | Cloud Build: `paperloom-manual` by hand, `paperloom-release` on tags | CircleCI, Trigger Pipeline with `build`; then Azure `target: linux-android` |
| Windows app (NSIS, MSI) and the core's Windows tests | Windows x64 | Azure: Run pipeline (default `target`), and on tags | none for now |
| iOS, iPadOS, macOS: build check, TestFlight, notarized DMG | arm64, x86_64 (universal Mac) | Xcode Cloud, by hand and on tags | none (the only Apple builder) |
| Android share and phone tests on an emulator, with screenshots | Android x86_64 (emulator) | CircleCI, Trigger Pipeline with `android_test` (Android machine image, `large`) | Azure, Run pipeline with `target: android-test` (its agents have KVM; 2 cores, 7 GB) |
| GitHub release | all of the above | each service adds its own files on a `v*` tag (`scripts/ci/github_release.py`) | |

How the free time is shared: CircleCI's credits go mostly to the checks
(the only automatic job); Cloud Build's minutes only to Linux and Android
packages; Azure's minutes mostly to Windows, with the rest as the fallback
for the other two; Xcode Cloud's hours only to Apple. GitHub Actions on
Blacksmith (`.github/workflows`) is parked until its account is verified:
every job there is skipped unless started by hand.

## Free allowances and what they buy (October 2026)

Measured on the first runs (2026-10-18); "runs a month" assumes nothing
else uses that service.

| Service | Free a month | Cost of one run | Runs a month |
| --- | --- | --- | --- |
| CircleCI | 30,000 credits | checks: core 3.5 min, server 1.6 min, ui 3.3 min on Docker large (20 credits/min), about 170 credits | about 170 pushes, or checks plus some fallback builds |
| Google Cloud Build | 2,500 build-minutes (e2-standard-2) | 68 min: Linux 33, Android 34, plugins 1 (no cache between builds) | about 35 builds |
| Azure Pipelines | 1,800 minutes, one job at a time, 60 minutes a job | Windows: tests 10 min, app 30 min | about 44 Windows builds |
| Xcode Cloud | 25 compute hours | CI about 12 billed minutes; TestFlight (two archives) 13; Mac Direct 8 | about 45 rounds of all three |
| Blacksmith (parked) | 3,000 2-vCPU minutes (4 vCPU counts double, Windows double again) | not used | |

Limits that matter: CircleCI and Azure end jobs after 60 minutes; release
builds use thin LTO with one codegen unit, so the Windows app is the job
most likely to reach it on Azure's 2 vCPUs. Cloud Build has no such limit.
Blacksmith runs jobs only for the `routeforge-dev` organization; as of
2026-10-18 its account still needs the additional verification its
dashboard asks for (Contact support), and until then it picks up no jobs,
so it is parked.

Keep the Xcode Cloud CI workflow manual (or limited to changes under
`apple/`): every action rebuilds the Rust core and the hours are shared
with TestFlight and the DMG.

## Licences and third-party notices

Paperloom is closed source, so its dependencies may not carry copyleft that
would reach its own code. The core check runs cargo-deny 0.20.2 (the release
binary, pinned by checksum in `scripts/ci/run.sh`) against `deny.toml`:
`licenses bans sources` must pass (allowed licences listed there; GPL, AGPL
and LGPL are not; MPL-2.0 is, file by file), and so must `advisories`: a new
security advisory fails the check until the crate is updated or the
advisory is listed in `deny.toml` with the reason it is accepted for now. The ui check runs
`scripts/licenses.py check-npm`: every npm package the web UI ships
(`app/package-lock.json`, dev dependencies left out) against the same list.

Every app build writes THIRD_PARTY_NOTICES (`scripts/licenses.py notices`:
each Rust crate and shipped npm package, its licence and the licence texts
it carries, each text once): into the web UI (`app/public/`) for the Tauri
apps, and into the Apple app's resources in `ci_post_clone.sh` (Rust crates).
Settings > About shows it. The Apple privacy manifest is
`apple/Paperloom/Resources/PrivacyInfo.xcprivacy`; the draft privacy policy
is `docs/privacy.md`.

## Testing on emulators and real installs

Nobody keeps test devices at hand, so a change that needs a device check is
installed and driven on a cloud machine, and the screenshots are looked at
before calling it done.

- Android: CircleCI, Trigger Pipeline with the boolean parameter
  `android_test` = true (the dialog lists the parameters only after the
  config and checkout branches are picked), or Azure, Run pipeline with
  `target: android-test` (one job builds the APK, the next runs the emulator
  with the agent's 2 cores; artifact `android-test`). The job builds an x86_64 APK,
  starts an Android 14 emulator (Pixel 6, 1080×2400, 420 dpi) on the Android
  machine image (it has KVM; Gen1 `large` is on the free plan), installs the app and runs
  `scripts/ci/android-share-test.sh`: it checks that the share sheet lists
  Paperloom, then shares a DOI (starting the app), an arXiv link, a
  publisher's page, an RIS file, a PDF, and a scanned PDF (a picture of a
  page showing the first DOI, drawn with Pillow): read on the phone (ML
  Kit), its DOI makes it join that paper. After each step the job keeps a
  screenshot and the text on screen (`NN-step.png`, `NN-step.txt`, read
  through uiautomator) under `test-results/android/`, stored as the artifacts'
  `android-test/`.
  Steps that depend on a third-party site, or on handing a file over through
  the media store, warn instead of failing.
  The same emulator then runs `scripts/ci/android-phone-test.sh`: Library,
  an item and Android Back, the newest item's PDF in the reader (pdf.js has
  to open it in this WebView: its page count shows), Search Everything, More → Online Search,
  Settings (also scrolled) and Browser, Notes with a new mind map, canvas,
  diagram and Graph, and Add Item dismissed with Android Back. The phone
  Notes menu uses Insert Diagram from a new note when New Diagram is hidden.
  These screenshots, text and XML dumps use `phone-NN-step` names. Missing
  controls on a readable screen fail; missing Notes entry points or incomplete
  WebView dumps warn for review. Back at the library root should leave the
  app; staying in front warns. Each phone shot also records right-edge
  overflow in `phone-overflow.txt`; overflow only warns, including allowed
  sideways source-chip rows. One `summary.txt` covers both parts, alongside
  `logcat.txt` and `emulator.log`. The shared setup and screen helpers are in
  `scripts/ci/android-emulator.sh`; only `scripts/ci/run.sh android-test`
  starts the emulator.

- Desktop layout: CircleCI, Trigger Pipeline with `desktop_test` = true, or
  Azure, Run pipeline with `target: desktop-test` (artifact `desktop-test`). The
  job builds the web UI and opens it (`?demo`, the in-memory library with
  samples) in Chromium, as Windows' WebView2 draws it, and WebKit, as Linux's
  WebKitGTK and Safari do, at 1440×900, 1280×800, 1024×768 and 800×700, and
  again in Simplified Chinese and dark mode at 1280×800, 800×700 and 390×844
  (the phone layout the Android app uses) (`scripts/ci/desktop-screens.mjs`,
  Playwright 1.63 installed for the job only). It walks Library, an item, the split view, Search Everything, Online
  Search, Browser, Notes and a new note, Settings (also scrolled) and the Add
  Item dialog, and keeps a screenshot of each with `report.md`: controls past
  the window, cut-off text, parts hidden by a clipping parent, and overlapping
  controls (within the dialog when one is open). In Chromium at 1280 and 390
  wide every screen is also audited with axe-core 4.10 (WCAG 2.2 AA and axe's
  best practices): `accessibility.md` lists each broken rule once, with the
  screens and elements it names. These warn; the job fails when the
  app does not load or the page throws. Artifacts: `desktop-test/`.
- Windows: every Azure run of the `windows` target installs the NSIS
  installer it built (silently), starts Paperloom and keeps screenshots of the
  screen and of its window after 15 and 30 seconds, with `summary.txt`
  (`scripts/ci/windows-smoke.ps1`); artifact `windows-screens`. It fails when
  the installer fails or the app is gone within 30 seconds. It also draws a
  line of text as a picture and has the installed app read it
  (`paperloom-app.exe --recognize-text`, Windows' own text recognition); the
  summary notes what was read, or that the agent has no recognizer.
- Linux app end to end: Azure, Run pipeline with `target: desktop-e2e`
  (one job builds the .deb, about 25 minutes; with `appRun` = an earlier
  run's id it takes that run's `linux` packages instead). The test job
  installs the .deb, starts it in a virtual display with a fresh home
  through tauri-driver and WebKitWebDriver (`TAURI_WEBVIEW_AUTOMATION`),
  and `scripts/ci/desktop-e2e.mjs` walks it with its real library, files
  and network: add a paper by DOI; add a scanned PDF (no text, only a
  picture showing the DOI and a line about a lighthouse keeper), which
  Tesseract reads so it joins that paper; open it in the reader and
  Recognize Text; find "lighthouse" with Search Everything; write a note
  and find its file; Cite in Word Document on a .docx citing the DOI and
  check the copy. The GTK file dialogs are answered with xdotool (the app
  reads only files picked in a dialog). Screenshots, page text,
  `summary.txt`, `driver.log` (the app's output) and the cited copy are the
  artifact `desktop-e2e`.
- Windows app end to end: Azure, Run pipeline with `target: windows-e2e`
  (one job builds only the NSIS installer; `appRun` takes an earlier run's
  `windows` installers instead). `scripts/ci/windows-e2e.ps1` installs it
  silently, downloads msedgedriver of the WebView2 runtime's own version
  (msedgedriver.microsoft.com), starts the app with WebView2's DevTools port
  open (`PAPERLOOM_WEBVIEW2_DEBUG_PORT`, which the app adds to its own
  WebView2 switches: WebView2's environment variable and policy never
  reached the browser process), attaches msedgedriver to it
  (`debuggerAddress`; launched through tauri-driver the app never showed the
  port) and runs the same `desktop-e2e.mjs` walk; Windows' own text recognition reads the
  scanned page, and the Open / Save As dialogs are answered through UI
  Automation (`scripts/ci/windows-dialog.ps1`: the File name box and the
  Open or Save button by their ids), with a screenshot of the screen at
  each. The RIS file is opened through the shell (`start`), as a double
  click in Explorer does, so the installer's file associations are tested
  too; the log lists what `assoc` says for .ris, .enw and .nbib. The
  installer is built with a stand-in Word add-in address
  (`PAPERLOOM_WORD_ADDIN_ORIGIN`), so Settings installs the Word add-in: the
  walk checks Word's developer add-in entry in the registry and the
  manifest it names, then removes it. Artifact `windows-e2e`.
- WPS for Linux: Azure, Run pipeline with `target: wps-e2e`.
  `scripts/ci/wps-e2e.sh` installs the China personal build (its download
  page signs links with a key in the page; the script does the same and
  checks the file), accepts its licence agreement in `Office.conf`,
  puts the probe add-in (`scripts/ci/wps-probe/`) in the jsaddons folder,
  online and offline, and opens a document in a virtual display. The probe
  reports each document operation the WPS add-in needs to a local server
  (`reports.jsonl`); if no add-in loads, it tries again with `JsApiPlugin`
  on in `oem.ini`, then with a JS plugins server. Artifact `wps-e2e`
  (reports, screenshots, the saved documents' XML, `summary.txt`). With
  `appRun` (an earlier run's `linux` packages, e.g. from `desktop-e2e`) it
  tests the real add-in instead: the app, through tauri-driver, adds a
  paper by DOI and installs the add-in from Settings; WPS starts in the
  same home, the first-start questions are clicked where they show, and
  `scripts/ci/wps-probe/pane-e2e.js` (added to the installed pane) connects,
  turns the document's `[@DOI]` marker into a citation, cites two papers
  (one added by DOI, one opened from a RIS file), inserts the reference
  list right after them and saves, while WebDriver allows the connection in
  the app. The saved document must hold three citation fields with only
  "[1]", "[2]" and "[1]" (superscript), no marker, the reference list with
  both papers in paragraphs of its own, and Paperloom's data.
- Linux, real programs on real files: Azure's `checks` core job sets
  `CI_REAL_FILES`, and `scripts/ci/real-files.sh` installs Tesseract (English
  and Simplified Chinese), LibreOffice Writer and a Chinese font. The OCR
  crate's test reads a picture it drew (`OCR_SAMPLE_PNG`: an English and a
  Chinese line) with Tesseract; the citation tests write a Word and an
  OpenDocument paper and their cited copies (`WORDCITE_SAMPLES`), and
  LibreOffice converts the copies to HTML, which must show the numbered
  citations, the footnote's citation and the reference list, and no `[@…]`
  or placeholder left. CircleCI's core job runs the same tests without these
  files (they then check nothing). The picture, the documents and the HTML
  are the artifact `real-files`.
- Mac: run Xcode Cloud's Mac Direct workflow, take the link to its
  `STAPLED_NOTARIZED_ARCHIVE` artifact from the App Store Connect API
  (`/v1/ciBuildActions/{id}/artifacts`, valid for a short while), and run the
  Azure pipeline with `target` = `macos` and `macAppUrl` = that link. A
  macOS 26 machine downloads the app, checks the signature and Gatekeeper's
  verdict, copies it to /Applications (without a quarantine flag: nobody
  could answer the first-open question there), opens it and keeps
  screenshots and `summary.txt` (`scripts/ci/macos-smoke.sh`); artifact
  `mac-screens`.
- iPhone and iPad: start Xcode Cloud's **UI Screens** workflow. The UI test
  `apple/ScreensUITests` starts the app on an iPhone 17 and an iPad Pro
  13-inch simulator with `-PaperloomScreenshots` (sample papers, notes and
  a small Obsidian plugin in a temporary folder made afresh at each launch),
  walks the main screens once in English and once in Simplified Chinese in
  dark mode, and attaches a screenshot and the accessibility tree (element
  frames, `.txt`) for each. The iPhone walk also opens a RIS file and a PDF
  in the app as another app would, and types in a note to call up the
  plugin's suggestions. A screen it cannot reach fails the test once the others are
  photographed. Take the `RESULT_BUNDLE` artifacts' links from the App Store
  Connect API as for the Mac, and copy the pictures out with
  `scripts/ci/xcresult-attachments.py <zip> <folder>` (no Xcode needed).

CircleCI pipelines can be started and followed with `scripts/ci/circleci.sh`
(a personal API token in the Keychain as `circleci-token`). Xcode Cloud can
be driven from a terminal with `scripts/ci/asc.py`
(start a workflow on a branch, follow a run, and `screens` to copy the UI
tests' screenshots out of the result bundles), given a team App Store
Connect API key in `~/.appstoreconnect/` (never committed).

## Secrets

| Secret | CircleCI (project env vars) | Cloud Build (Secret Manager) | Azure (pipeline variable) | Xcode Cloud (workflow env, secret) | GitHub (repo secrets) |
| --- | --- | --- | --- | --- | --- |
| `ANDROID_KEYSTORE_B64`, `ANDROID_KEYSTORE_PASSWORD`, `ANDROID_KEY_ALIAS` | yes | required | for `linux-android` | | yes |
| `GITHUB_RELEASE_TOKEN` | releases | required | releases | releases (Mac Direct) | optional |
| `APP_STORE_CONNECT_KEY_ID`, `APP_STORE_CONNECT_ISSUER_ID`, `APP_STORE_CONNECT_PRIVATE_KEY` | | | | Mac Direct | |
| `DEVELOPER_ID_P12_B64`, `DEVELOPER_ID_P12_PASSWORD` | | | | Mac Direct, optional (signs the DMG) | |

`GITHUB_RELEASE_TOKEN` is a fine-grained personal access token for this
repository only, with Contents: Read and write. Without the Android
secrets CircleCI, Azure and Blacksmith sign the APK with a throwaway key
and name it `-test`.

The Android key in the secrets since 2026-10-18 is a development key
(alias `paperloom`, kept outside the repository by the owner). Replace it
with the real release key before the first public Android release: an app
signed with one key cannot be updated by an APK signed with another.

## Setting up each service

### Xcode Cloud

The Xcode project is generated, not committed: `apple/ci_scripts/ci_post_clone.sh`
installs Rust and XcodeGen, builds the Rust core for the action's platform
and generates the project; `ci_post_xcodebuild.sh` makes the DMG.

Set up 2026-10-18: Xcode Cloud product `Paperloom-iOS` (the app
`io.felixbennett.paperloom`), connected to `routeforge-dev/paperloom`
through the Xcode Cloud GitHub app.

1. The product was created from Xcode: `cd apple && xcodegen generate`
   (needs `CoreKit/Frameworks/PaperloomCoreFFI.xcframework` to exist, an
   empty placeholder with an `Info.plist` will do, or Xcode never finishes
   loading the package), open `Paperloom.xcodeproj` (no need to build),
   Report navigator > Cloud > Get Started.
2. Workflows are edited in App Store Connect > Xcode Cloud > Workflows,
   except for picking the scheme of a new action: the project is generated,
   so only Xcode on the Mac knows the schemes (Integrate > Manage
   Workflows). Duplicating a workflow on the web keeps its schemes.
3. Workflows (Xcode and macOS "Latest Release", Xcode 27 as of October
   2026; clean builds off, so derived data is cached):
   - **CI**: manual start only, any branch. Actions: Build `Paperloom-iOS`
     (any iOS device) and Build `Paperloom-macOS` (any Mac; the post-clone
     script also runs the CoreKit tests). About 6 minutes each.
   - **UI Screens**: manual start only, any branch. Action: Test
     `Paperloom-iOS` (scheme test target `Paperloom-iOSScreens`) on the
     iPhone 17 and iPad Pro 13-inch (M5) simulators, iOS 27. Created
     through the App Store Connect API (`POST /v1/ciWorkflows`). The scheme
     tests through `apple/PaperloomScreens.xctestplan`, whose
     `diagnosticCollectionPolicy: Never` stops xcodebuild collecting
     diagnostics after the run (it waited ten minutes per simulator, even
     when the tests passed); `ci_post_clone.sh` writes the generated
     target's id into the plan. About 15 minutes a run.
   - **Store Screens**: manual start only, any branch. The App Store's
     pictures: Test `Paperloom-iOS` with the test plan
     `PaperloomStore` (only `StoreScreens`) on the iPhone 17 Pro Max
     (6.9-inch, 1320 × 2868) and iPad Pro 13-inch (M5) (2064 × 2752)
     simulators, and Test `Paperloom-macOS` (test target
     `Paperloom-macOSScreens`, plan `PaperloomStoreMac`) on a Mac, whose
     app window is 16:10, as large as its screen allows (scale those
     pictures to 2560 × 1600: `apple/AppStore/README.md`). `apple/ScreensUITests/StoreScreens.swift`
     starts the app with `-PaperloomStore` as well (no test plugins or
     scanned page; a paper with a readable PDF and two highlights) and
     photographs the library, a paper, its PDF, a note and search in each
     of the six languages, light; the pictures are attachments named
     `store-<device>-<language>-<n>-<screen>`, copied out with `asc.py
     screens` like the UI Screens ones. Created through the API.
   - **TestFlight**: manual start (branches and tags) and tag changes
     starting with `v`. Actions: Archive `Paperloom-iOS` and Archive
     `Paperloom-macOS`, both prepared for App Store Connect, so the builds
     reach TestFlight.
   - **Mac Direct**: as TestFlight for starts; environment
     `PAPERLOOM_DIRECT=1` plus the Mac Direct secrets above. Action:
     Archive `Paperloom-macOS` with distribution preparation None (there is
     no Developer ID choice), and the post-action Notarize - macOS: only
     with it does Xcode Cloud export a Developer ID build, which
     `ci_post_xcodebuild.sh` takes from `CI_DEVELOPER_ID_SIGNED_APP_PATH`.
     A workflow with that post-action can only be edited by the Account
     Holder, Admins and App Managers. Without a tag the DMG is checked but
     not kept. The App Store Connect API key is a team key with Developer
     access. Secret workflow variables cannot be renamed: remove and add
     them again.
4. Build numbers continue from 1000 (TestFlight already had up to 391);
   Xcode Cloud only lets the next number go up.
5. Signing is cloud-managed. The Developer ID certificate needs the Account
   Holder, or an Admin allowed to use cloud-managed Developer ID
   certificates.

### CircleCI

1. CircleCI organization `RouteForge`, project `Paperloom`, connected to
   the CircleCI GitHub app on `routeforge-dev` (Organization Settings > VCS
   Connections). Its pipeline `ci` reads `.circleci/config.yml` and runs on
   all pushes. Leave "GitHub Actions workflow file" unticked: CircleCI
   would then read the file as a GitHub workflow.
2. Project Settings > Environment Variables: the secrets above.
3. Plan > Usage Controls: keep caches and artifacts for a few days, to stay
   within the 2 GB.

### Google Cloud Build

Project `paperloom-ci`, region us-east1 (set up 2026-10-18):

1. Cloud Build and Secret Manager are enabled; the bucket
   `paperloom-ci-artifacts` (us-east1, private) deletes objects after 14
   days.
2. Secret Manager: the four secrets above; the build runs as the default
   compute service account, which needs the Secret Manager Secret Accessor
   role on them.
3. Cloud Build > Repositories (2nd gen): connection `github-routeforge-dev`,
   repository `routeforge-dev-paperloom`.
4. Triggers using `ci/cloudbuild.yaml`: `paperloom-release` (tag push
   `^v.*`) and `paperloom-manual` (manual; pick the branch when running it).
   No schedules.
5. The billing account's $1 budgets cover every project and e-mail at 50%,
   90% and 100%; Cloud Build cannot be hard-capped.

### Azure Pipelines

1. Organization `routeforge` (directory 預設目錄), project `Paperloom`;
   Parallel jobs shows the free Microsoft-hosted job (1,800 minutes).
2. The Azure Pipelines GitHub app is installed on `routeforge-dev`; the
   pipeline `routeforge-dev.paperloom` uses `azure-pipelines.yml`.
3. Pipeline variables, marked secret: `GITHUB_RELEASE_TOKEN`, and the
   three Android ones for `target: linux-android`.
4. No schedule: tags build Windows; Run pipeline asks for `target`.

### Blacksmith (GitHub Actions, parked)

1. The repository lives in the free GitHub organization `routeforge-dev`
   (Blacksmith runs jobs for organizations only); install the Blacksmith
   GitHub app on it.
2. Repository secrets as above. Repository variables turn the automatic
   triggers on: `CHECKS_ON_GITHUB` (ci.yml on pushes and pull requests),
   `RELEASE_ON_GITHUB` (release.yml on `v*` tags); `nightly.yml` only ever
   runs by hand. Without them every job is skipped, so GitHub-hosted
   minutes are never used.

## Mirrors

Machines in China can name mirrors (`CI_CARGO_MIRROR`, `CI_APT_MIRROR`,
`CI_MAVEN_MIRROR`, `CI_GRADLE_DIST_MIRROR`; rustup's `RUSTUP_DIST_SERVER`);
`scripts/ci/` uses them when set and the upstream registries otherwise.
