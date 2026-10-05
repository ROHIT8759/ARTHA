# ARTHA — Offline Billing & Inventory Platform

Local-first, offline-capable billing and inventory for small businesses.
The owner's PC runs the server and database; staff connect over the shop's
Wi-Fi/LAN from a browser. No internet required for core operations.

Full product spec, data model, and the V0 → V1 → V2 → V3 roadmap: **[docs/SPEC.md](docs/SPEC.md)**.

## Status

**V0 (foundation)** is scaffolded:

- `backend/` — Rust (Axum) REST API + SQLite, with migrations, owner/staff
  auth (Argon2 password hashes, bearer session tokens), role-based
  authorization enforced server-side, product CRUD with QR identity, and
  an audit log. See [backend/README.md](backend/README.md).
- `frontend/` — Next.js + TypeScript shell: first-run setup, login,
  dashboard, and a product catalog screen with QR code assignment. See
  [frontend/README.md](frontend/README.md).

Not yet built: sales/billing (V0.1), purchase OCR (V0.2), autosave/backup/
restore (V0.3). See [docs/SPEC.md §5](docs/SPEC.md#5-version-roadmap) for
the full phase breakdown and [§18](docs/SPEC.md#18-recommended-build-order)
for build order.

## Running it locally

You need the Rust toolchain (`rustup`, plus the MSVC "Desktop development
with C++" workload on Windows) and Node.js (already installed — v24+).

```sh
# Terminal 1 — backend, listens on 0.0.0.0:8080 by default
cd backend
cargo run

# Terminal 2 — frontend dev server on :3000, talking to the backend above
cd frontend
npm install
npm run dev
```

Open http://localhost:3000 — it will route you to **Setup** on first run
(create the business + your owner account), then to **Login** afterwards.

To test LAN access the way staff devices will use it: find this PC's LAN
IP (`ipconfig`, look for the Wi-Fi/Ethernet adapter's IPv4 address) and open
`http://<that-ip>:8080` directly from a phone on the same Wi-Fi — the
backend serves the API there; wiring it to also serve the built frontend
(or running the frontend against that same host) is part of V1 packaging.

## Repository layout

```
backend/   Rust REST API + SQLite (source of truth)
frontend/  Next.js + TypeScript UI
docs/      Product spec and roadmap
```
