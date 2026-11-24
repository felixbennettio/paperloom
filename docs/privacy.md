# Paperloom privacy policy (draft)

Draft for the owner to review before release: the contact, the operator's
name and the Paperloom Cloud terms are placeholders. Keep it in step with
the Apple privacy manifest (apple/Paperloom/Resources/PrivacyInfo.xcprivacy)
and the App Store privacy labels.

## In short

Paperloom keeps your library, PDFs, annotations and notes on your own
devices. It has no advertising, no analytics and no tracking. It sends
data only when you ask for something that needs another service, and only
to that service.

## What stays on your device

- The library (references, collections, tags, annotations), the PDFs and
  other files you add, and your notes vault.
- Settings, including API keys and passwords for services you connect;
  secrets are kept in the system's keychain or credential store.
- Search indexes and, if you turn on semantic search, the vectors computed
  for it (a file next to your library).

## When Paperloom contacts other services

Each of these happens because you used the feature, and sends only what
the feature needs:

- **Looking up references.** Adding a paper by DOI, arXiv ID, PMID or ISBN,
  recognising a PDF, or searching online sends the identifier or your
  search words to the matching service: Crossref and doi.org, OpenAlex,
  arXiv, PubMed (NCBI), Semantic Scholar, Open Library, library catalogues
  you add (SRU and Z39.50, such as the Library of Congress, K10plus or the
  DNB), and databases you configure with your own key (such as Elsevier,
  IEEE or Springer). Their own privacy policies apply.
- **Web pages.** The built-in browser loads the pages you open; reading a
  page's references happens on your device. Zotero translators run on your
  device and are downloaded from GitHub only when you ask for them.
- **AI features.** When you ask the AI something, the question and the text
  it needs (such as a passage, an abstract or search results from your
  library) go to the AI provider you set up, with your own API key. Nothing
  is sent while no provider is set up.
- **Sync and backup.** Your library and files go to the place you choose:
  your own S3, WebDAV, Seafile or iCloud storage, a local folder, or
  Paperloom Cloud.
- **Plugins.** Obsidian and Zotero plugins run in a sandbox; a plugin
  reaches a website only after you allow that site.
- **Text recognition.** Scanned pages are read on your device (Apple's
  Vision, Windows' own recognizer, Tesseract on Linux, ML Kit on Android).
- **Browser extension.** The extension talks to the app on your computer
  only (a local port).

## Paperloom Cloud (if you sign in)

Signing in creates an account with your email address and an account ID.
If you turn on Paperloom Cloud sync, your library and files are stored on
Paperloom's servers to sync them between your devices. They are used for
nothing else. [Placeholder: where the servers are, how long data is kept,
how to delete the account.]

## Your choices

You can use Paperloom without an account and without any online service.
You can delete everything by deleting the library and the vault, and your
Paperloom Cloud account from the account settings [placeholder].

## Contact

[Placeholder: the operator's name and a contact address.]
