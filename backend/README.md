# ARTHA backend

Rust (Axum) REST API over a single SQLite database file. See
[../docs/SPEC.md](../docs/SPEC.md) for the full product spec; this file is
just "how to run and extend what's here."

## Prerequisites

- Rust via [rustup](https://rustup.rs/).
- On Windows: the MSVC build tools (Visual Studio Build Tools, "Desktop
  development with C++" workload), needed by `rusqlite`'s bundled SQLite
  and by `ring`/`argon2`'s dependency chain.

## Run

```sh
cargo run
```

Env vars (all optional — see `src/config.rs`; no cloud configuration exists
to set):

| Var | Default | Meaning |
|---|---|---|
| `ARTHA_HOST` | `0.0.0.0` | interface the server listens on |
| `ARTHA_PORT` | `8080` | port the server listens on |
| `ARTHA_DB_PATH` | `data/artha.db` | SQLite file location (created if missing) |
| `ARTHA_LOG_LEVEL` | `info` | tracing log level, e.g. `debug` |
| `ARTHA_ENV` | `development` | `development` or `production` — controls log formatting (ANSI colors in dev, plain in prod) |
| `RUST_LOG` | — | if set, overrides `ARTHA_LOG_LEVEL` with the usual `tracing_subscriber` filter syntax, e.g. `artha_server=debug` |
| `ARTHA_UPLOAD_DIR` | `uploads/` next to `ARTHA_DB_PATH` | where purchase bill photos are stored (content-addressed: `<sha256>.<png\|jpg\|webp>`). Included in backups — see [Bill photos in backups](#bill-photos-in-backups). |
| `ARTHA_BACKUP_DIR` | `backups/` next to `ARTHA_DB_PATH` | where backups (and the shared copy of bill photos, `images/`) are kept |
| `ARTHA_PADDLE_OCR_CMD` | — | path to a single executable run as `<cmd> <image-path>` that prints a JSON array of `{text, confidence}` tokens on stdout. Arguments cannot be embedded in the value; use a wrapper script. |
| `ARTHA_OCR_TIMEOUT_SECS` | `30` | wall-clock budget for one OCR run, including time spent waiting for a free OCR slot |
| `ARTHA_OCR_MAX_CONCURRENCY` | `2` | how many OCR engine processes may run at once (1–16). Requests beyond that wait, then get a "busy, try again" (HTTP 503) |
| `ARTHA_OCR_USE_DEV_FIXTURES` | `false` | development only: when **no** OCR engine is configured, return made-up sample bill data instead of an error. Accepts `true/false/1/0/yes/no/on/off`; anything else is a startup error. **Ignored in production.** |

**OCR without an engine.** When `ARTHA_PADDLE_OCR_CMD` is unset, OCR requests fail with a
clear message telling the user to enter the bill by hand. Sample data is returned only if you
*explicitly* set `ARTHA_OCR_USE_DEV_FIXTURES=true` **and** `ARTHA_ENV=development`; the
response then has `is_dev_fixture: true` and `engine_version: artha_ocr_dev_fixture_v1`, the
purchase is permanently flagged (`ocr_is_dev_fixture`) and the screen shows a warning that
survives reloads and retries. With `ARTHA_ENV=production` the flag has no effect (the server
logs a warning at start-up). The default `ARTHA_ENV` is still `development`, so production
installs must set `ARTHA_ENV=production`.

**OCR process control.** The engine runs in its own process group (Unix) or Job Object
(Windows) via the `process-wrap` crate. On timeout, oversize output, or a dropped request the
whole process tree is killed (before the leader is reaped, so no recycled process id can be
signalled). Output is bounded: stdout is capped at 4 MiB (more is rejected and the engine is
killed); only the first 16 KiB of stderr is kept for the server log. Client-facing errors
never contain executable paths, OS errors, tracebacks or engine output. Limitations: a
descendant that deliberately leaves the process group/session (Unix) or breaks away from the Job
Object (Windows) cannot be reached. A descendant that merely outlives the engine keeps the run
open until the time budget ends, then the tree is killed. The Windows path is exercised by the
Windows CI job, not by the Linux job or by a local Linux run.

**What the bill parser understands.** One OCR line per item: `<name> [<qty>] <unit price>
[<gst>%]`. A bare number in the quantity position (`Soap 3 350.00 5%`), an explicit label
(`Qty 3`, `Qty:3`) and a count unit (`12 pcs`, `12pcs`) are read as the billed quantity.
Weight/volume text next to the name (`Rice 5kg`, `Oil 1L`, `5 kg`) is treated as a **pack
size**, not a quantity: the line is suggested with quantity 1 and flagged
(`quantity_certain: false`, `pack_size`, `quantity_note`); the same applies to `2 x 1kg` and to
a number that looks like an amount (`100.00`). The review screen will not allow approval until
each flagged quantity is confirmed or edited. Lines without a readable name/quantity/price are
skipped, not guessed. Quantities are exact integer thousandths (at most 3 decimals, up to
1,000,000 units); unit prices are capped at 1e11 paise. This is a heuristic for simple
single-line invoices, not a general invoice reader. OCR output is advisory: it never approves a
purchase or changes stock.

### Bill photos in backups

Photos are immutable and content-addressed, which is what makes backing them up safe.

- **Backup** = the SQLite snapshot (`artha-backup-<id>.sqlite3`, unchanged format) + a copy of
  every photo that snapshot refers to in the shared store `<ARTHA_BACKUP_DIR>/images/` + a
  manifest `artha-backup-<id>.images.json` bound to the snapshot's SHA-256. The manifest is
  written atomically *after* the photos, and the backup is only registered once all of that
  succeeded; a failure removes the partial files and registers nothing. A photo whose live
  file is missing or damaged is **reported** (`images_missing`), never silently omitted.
- **Restore** order: verify the database backup → verify the manifest and re-hash every photo
  against the snapshot (any mismatch, missing file, symlink, duplicate, or unexpected name
  refuses the restore with nothing changed) → safety backup of the current state → copy
  photos into `ARTHA_UPLOAD_DIR` (additive; an existing good file is left alone, a damaged one
  is renamed `*.corrupt-<id>`, never deleted) → restore the database. The database is therefore
  never restored without its photos. Photos are located by validated file name inside
  `ARTHA_UPLOAD_DIR`, so a restore onto another machine or folder works.
- **Legacy (database-only) backups** have no manifest. They still verify and restore exactly as
  before; the restore response reports `images.missing`, the number of photos the database
  refers to that are not on disk.
- **Retention** deletes a pruned backup's own files (database copy and manifest). The shared
  photo store is **never** pruned automatically, so it grows with the number of distinct photos
  (follow-up: garbage-collect photos no retained backup refers to).
- `POST /api/backups/<id>/verify` fully re-hashes the photos (`images_ok`); create-time only
  trusts an existing stored copy of the same size, for speed.


The database is created and migrated automatically on first start. There
is no separate "init" step — `POST /api/setup` (called once by the
frontend's first-run screen) creates the business and owner account.

## Test

```sh
cargo test
```

- `tests/foundation.rs` — infrastructure layer: app startup, database
  init + migration execution, the health endpoint, configuration loading.
- `tests/smoke.rs` — the full V0 feature flow: setup → login → RBAC
  rejection → product CRUD → QR lookup.
- Each module also carries its own unit tests (`src/config.rs`, `src/db.rs`).

## Module map

| Module | Responsibility |
|---|---|
| `config` | `Config`: host/port/db path/log level/dev-or-prod, loaded from env vars |
| `logging` | one-time `tracing` subscriber setup, driven by `Config` |
| `db` | connection setup (WAL, foreign keys), migration runner |
| `auth` | password hashing (Argon2), session tokens, the `AuthUser` extractor that enforces auth + role on every protected handler |
| `audit` | append-only log of sensitive actions |
| `models` | request/response DTOs shared with the frontend |
| `routes::health` | liveness probe, no auth |
| `routes::setup` | first-run business+owner creation, login/logout/me |
| `routes::users` | owner-only staff account management |
| `routes::products` | product CRUD + QR lookup |

Each future module (`inventory`, `purchases`, `sales`, `ocr`, `parser`,
`backup`, `update`, `reporting` — see [SPEC.md §14](../docs/SPEC.md#14-api--module-boundaries))
should follow the same shape: a `routes::<name>` file owning its own SQL,
taking `AuthUser` for authn/authz, and recording audit events for anything
sensitive.

## Design notes (the "why" behind a few choices)

- **IDs are UUIDv4, not autoincrement.** Keeps them stable across export/
  import, backup/restore, and future multi-location sync (spec §4, §20).
- **Money is integer paise, quantity is integer milli-units.** Avoids
  floating-point drift in totals; the frontend is the only place that
  formats to rupees/decimal units for display.
- **One `rusqlite::Connection` behind a `Mutex`, not a pool.** SQLite
  serializes writers internally anyway, and a single shop PC's request
  volume is low. Revisit only if V1 load testing shows contention.
- **Role permission granularity (which staff can do what) is a V1
  feature.** V0 only distinguishes owner vs. staff for admin actions
  (staff management, user status changes); product/purchase/sale CRUD is
  open to any authenticated user for now.
