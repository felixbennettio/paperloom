# Sync protocol and Paperloom Cloud

Paperloom syncs a library by sharing a log of changes. The same log works
with the official service (Paperloom Cloud, `server/`) and, later, with
storage people bring themselves (S3, WebDAV, a folder, iCloud Drive). The
wire types are in `core/crates/sync-proto`.

## The model

- A **library** has a log of **changes**. A change is opaque bytes (an
  encoded Loro CRDT update in the apps); the service never reads it, so a
  change can be end-to-end encrypted.
- Each **device** numbers its own changes in a library 1, 2, 3… (`seq`). It
  keeps unsent changes and pushes them in order; sending one again is
  harmless.
- The service gives each change it keeps a **cursor**: its place in the
  library's log, from 1. Pushes are kept whole or not at all, and cursors
  follow the order pushes are kept in, so a device that has read up to
  cursor *n* has everything up to *n*.
- **Blobs** (attachments, snapshots) are named by the BLAKE3 hash of their
  bytes: 64 lowercase hex digits. With end-to-end encryption, that is the
  hash of the encrypted bytes.
- A **snapshot** is the whole library up to a cursor, in one blob. Setting
  one drops the changes it covers; a device behind it loads the snapshot,
  then reads on from its cursor.

## Accounts

Paperloom accounts come from Logto (OIDC). Devices sign in with
authorization code + PKCE and ask for an access token for the API resource
`https://sync.paperloom.routeforge.zone` with the scopes `sync:read` and
`sync:write`. The service checks the token against the issuer's published
keys (JWKS): signature, issuer, audience and expiry. It knows accounts only
by the token's `sub` and keeps no passwords.

Every request but `/health` carries `Authorization: Bearer <token>`.

## API v1

JSON bodies; bytes inside JSON are base64. Errors are
`{"error": "<code>", "message": "…"}` with the HTTP status:
`unauthorized` 401, `forbidden` 403, `not_found` 404, `conflict` 409,
`bad_request` 400, `too_large` 413, `internal` 500.

| Request | Scope | What |
| --- | --- | --- |
| `GET /v1/me` | read | `{sub, plan, devices, usage}` |
| `POST /v1/devices` `{id, name, platform}` | write | Registers a device (its ID is made once on the device); again renames it. 409 if the ID belongs to another account |
| `GET /v1/libraries` | read | The libraries the account is a member of: `{id, name, role, head, createdAt}` |
| `POST /v1/libraries` `{id, name}` | write | Makes a library (the ID can be made offline); again returns it. 409 if the ID is taken |
| `GET /v1/libraries/{id}` | read | One library |
| `POST /v1/libraries/{id}/changes` `{changes: [{device, seq, payload}]}` | write | Appends changes: `{head, cursors}`, a cursor per change (an earlier one for a change sent before, 0 if a snapshot has folded it in). 409 with `expectedSeq` when a device's numbers skip; nothing is kept then |
| `GET /v1/libraries/{id}/changes?after=<cursor>&limit=<n>` | read | `{changes: [{cursor, device, seq, payload}], head, more, snapshot}`, oldest first, at most 500. When `snapshot` is set, load it and ask again after its cursor |
| `GET /v1/libraries/{id}/events` | read | Server-sent `head` events `{"head": n}`: the head now, then whenever it grows |
| `POST /v1/libraries/{id}/snapshot` `{cursor, blob}` | write | Folds the log up to `cursor` into the blob (uploaded first) |
| `PUT /v1/libraries/{id}/blobs/{hash}` | write | Stores a blob; 400 if the bytes do not hash to the name |
| `GET /v1/libraries/{id}/blobs/{hash}` | read | The blob's bytes |
| `GET /health` | none | `ok` |

Limits: 500 changes a push, 8 MiB a change, blobs up to
`PAPERLOOM_MAX_BLOB_MB`. Viewers can read a library but not change it.

Blobs belong to a library rather than an account, so everyone in a shared
library can read them, and nobody can learn whether another library holds a
given file.

## A device's sync loop

1. Register the device once; make the library on the service if it is new.
2. Push unsent changes in `seq` order, up to 500 at a time. On 409, resend
   from `expectedSeq`.
3. Pull after the last cursor read until `more` is false, applying each
   change (or the snapshot first), then remember the new cursor.
4. Listen to `events` and pull when the head passes the cursor; poll now
   and then as well, in case the connection drops.
5. Upload blobs before pushing changes that refer to them.

## In the apps (the sync engine)

The library stays a SQLite file; sync adds a Loro document beside it that
holds the same records as CRDT maps, so concurrent edits merge field by
field instead of one copy winning (`core/crates/sync`):

- root maps `tag`, `collection`, `item`, `attachment` and `annotation`,
  keyed by ID; each record is a mergeable map of its fields: an item's
  bibliographic data one field per key (`data.title`), its tags and
  collections one field per member (`tag:<name>`, `in:<collection id>`),
  so they merge as sets. A deletion is a `$gone` field and wins over
  edits made elsewhere at the same time;
- SQLite triggers note which records change (`sync_changed`); the engine
  copies only the fields that differ into the document, and turns this
  device's new writes into its next change;
- changes pulled from the log are imported into the document, and the
  records they name are written back to SQLite with the triggers off.
  Changes made here meanwhile go into the document first, so none is
  overwritten;
- an attachment's record names its file's blob (`blob`); files go up
  before the changes that name them, and files this device lacks come
  down after each sync;
- the document and the sync state are one file, `sync/state.bin`,
  replaced whole after each step, and changes are forgotten in SQLite
  only after it is written;
- notes come next: each note gets a Loro text, kept in step with its
  `.md` file.

The apps call `PaperLibrary::sync_with_cloud` with an access token for
the sync service (`AccountService::refresh(…, for_sync: true)`). A
device's first sync registers it and joins the account's library (or
makes it), then sends the whole local library; two devices that both had
the same paper keep both copies.

## Storage people bring

A library can also sync through a folder other devices reach (iCloud
Drive, a synced Nextcloud, Seafile or Syncthing folder, a NAS share), a
WebDAV folder (坚果云, Nextcloud, Synology) or an S3 bucket (AWS, Cloudflare
R2, MinIO, 阿里云 OSS, 腾讯云 COS), with no account
(`PaperLibrary::sync_with_folder`, `sync_with_webdav`, `sync_with_s3`).
S3 requests are signed with Signature Version 4; AWS, OSS and COS get the
bucket in the host name, other services in the path. There is no service
to number the log, so:

- each device writes its own changes as files that are never touched
  again, `changes/<device>/<seq, 20 digits>.change`, holding the same
  bytes as a change sent to the service; attached files are
  `blobs/<first two>/<hash>`;
- each device remembers how far it has read every other device's changes
  (`positions` in the sync state) and reads on from there;
- a change that builds on one not there yet (a synced folder still
  bringing it) waits, with the rest of its device's, and is taken in once
  that arrives; changes of other devices go on.

There are no snapshots here: a new device reads every change.

## End-to-end encryption

With a passphrase set on a device (`PaperLibrary::set_sync_passphrase`),
everything it sends is sealed with XChaCha20-Poly1305 before it leaves:
changes, and the files attachments name. The key comes from the passphrase
with Argon2id (64 MiB, three passes) and a salt each device makes once
(`sync/seal-salt`). Sealed bytes start `PLE1`, then the Argon2id memory
(KiB, u32 little-endian) and passes (u8), the salt, a 24-byte nonce and the
ciphertext; the bytes before the nonce are authenticated too. Another
device opens them with the same passphrase and the salt they carry, so
every device needs the same passphrase and nothing else is shared. A file
is sealed with a nonce made from its contents (a keyed BLAKE3 hash), so it
seals the same way each time and keeps its blob name; blobs are named by
the hash of the sealed bytes. The service and the storage see only sizes
and times.

A round reads the log before it sends: a device without the passphrase
stops at the first sealed change (and sends nothing), and a mistyped
passphrase fails to open another device's change before this device sends
anything sealed with it. Changes and files sent before the passphrase was
set stay as they were. The passphrase cannot be recovered or changed later
without starting the library's sync anew.

## Running the service

`paperloom-server` reads its settings from the environment:

| Variable | Default | |
| --- | --- | --- |
| `DATABASE_URL` | (required) | PostgreSQL; the schema is created or updated at start |
| `PAPERLOOM_BIND` | `0.0.0.0:8080` | Listen address (put TLS in the reverse proxy) |
| `PAPERLOOM_ISSUER` | `https://api.sso.routeforge.zone/oidc` | OIDC issuer of the tokens |
| `PAPERLOOM_AUDIENCE` | `https://sync.paperloom.routeforge.zone` | The API resource tokens must be for |
| `PAPERLOOM_BLOBS` | `fs:/var/lib/paperloom/blobs` | `fs:<folder>` or `s3` |
| `S3_ENDPOINT`, `S3_BUCKET`, `S3_REGION`, `S3_ACCESS_KEY_ID`, `S3_SECRET_ACCESS_KEY` | | With `s3` (R2, OSS, COS, MinIO…); region defaults to `auto` |
| `PAPERLOOM_MAX_BLOB_MB` | `512` | The largest blob |
| `RUST_LOG` | `info` | Log level |

`server/` is its own Cargo workspace with its own lockfile, so the
service's dependencies stay out of the apps' (`cargo test --manifest-path
server/Cargo.toml`). `server/Dockerfile` builds an image from the
repository root, and
`server/compose.example.yml` runs it with PostgreSQL and MinIO. It is not
deployed yet. When it is: the reverse proxy is added in 1Panel and points at
port 8080, and the Logto API resource above must exist.

Not yet: events between several instances (PostgreSQL LISTEN/NOTIFY; one
instance for now), presigned blob URLs for very large files, shared
libraries (members other than the owner), and subscriptions in `plan`.
