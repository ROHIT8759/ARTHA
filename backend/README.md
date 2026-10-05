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

Env vars (all optional):

| Var | Default | Meaning |
|---|---|---|
| `ARTHA_BIND` | `0.0.0.0:8080` | address the server listens on |
| `ARTHA_DB_PATH` | `data/artha.db` | SQLite file location (created if missing) |
| `RUST_LOG` | `info` | tracing log level, e.g. `debug`, `artha_server=debug` |

The database is created and migrated automatically on first start. There
is no separate "init" step — `POST /api/setup` (called once by the
frontend's first-run screen) creates the business and owner account.

## Test

```sh
cargo test
```

`tests/smoke.rs` drives the real router (in-memory SQLite) through the
core V0 flow: setup → login → RBAC rejection → product CRUD → QR lookup.

## Module map

| Module | Responsibility |
|---|---|
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
