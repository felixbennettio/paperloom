# App Store listing

What goes into App Store Connect for Paperloom (iOS, iPadOS and macOS share
one app record, `io.felixbennett.paperloom`). Nothing here is uploaded by
CI; the texts are entered (or sent through the App Store Connect API) by
hand, after review.

## Texts: `metadata/<locale>/`

One folder per App Store locale (en-US, zh-Hans, zh-Hant, fr-FR, ja, ru),
one file per field, with App Store Connect's limits:

| File | Field | Limit |
| --- | --- | --- |
| `name.txt` | Name | 30 characters |
| `subtitle.txt` | Subtitle | 30 |
| `promotional_text.txt` | Promotional Text (can change without a review) | 170 |
| `description.txt` | Description | 4,000 |
| `keywords.txt` | Keywords, comma separated | 100 |
| `release_notes.txt` | What's New (not shown for the first version) | 4,000 |

Keep them true to what the shipped build does: no feature that is not in
it (Paperloom Cloud, the Word and WPS add-ins), no other product's name in
the keywords (App Review guideline 2.3.7). The description may say which
plugins and file formats Paperloom works with.

## Pictures

The Xcode Cloud workflow **Store Screens** (docs/ci.md) photographs the
library, a paper, its PDF, a note and search in each language, on the
iPhone 17 Pro Max (6.9-inch: 1320 × 2868), the iPad Pro 13-inch
(2752 × 2064, landscape) and a 16:10 Mac window. Copy them out with
`scripts/ci/asc.py screens <run id> <folder>`; the files are named
`store-<device>-<language>-<n>-<screen>`. The cloud Mac's screen is too
short for a 1280 × 800 window, so its pictures are a smaller 16:10; scale
them to a size App Store Connect takes before uploading:
`sips -z 1600 2560 store-mac-*.png`.

## App Privacy (the "nutrition label")

Matches `apple/Paperloom/Resources/PrivacyInfo.xcprivacy` and
`docs/privacy.md`:

- Tracking: **No**.
- Data collected, all **Linked to the user**, **not used for tracking**,
  purpose **App Functionality** only, and only when someone signs in to a
  Paperloom account:
  - Contact Info > Email Address
  - Identifiers > User ID
  - User Content > Other User Content (the library synced through
    Paperloom Cloud)
- Everything else (the library, PDFs, notes, AI questions sent with the
  person's own key to their own provider, sync to their own storage) is not
  collected by the developer.

## Other answers

- Category: Education (`LSApplicationCategoryType`); secondary:
  Productivity.
- Age rating: no objectionable content; the built-in browser gives
  **Unrestricted Web Access**, which puts the app in App Store Connect's
  oldest age band unless the browser is limited.
- Export compliance: `ITSAppUsesNonExemptEncryption` is false (only the
  system's HTTPS).
- Sign in: an App Review account is not needed; every feature works
  without one.

## Still needed from the owner

- Support URL and privacy policy URL (`docs/privacy.md`, once its
  placeholders are filled and it is hosted).
- Copyright line (for example "2026 <name>"), price and the countries to
  sell in.
- Review of the texts, then entering them and submitting for review.
