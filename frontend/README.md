# ARTHA frontend

Next.js (App Router) + TypeScript UI, talking to the Rust backend's REST
API. See [../docs/SPEC.md](../docs/SPEC.md) for the product spec.

## Run

```sh
npm install
npm run dev
```

Opens on http://localhost:3000. By default it talks to the backend on the
same hostname, port 8080 (see `lib/api.ts`) — override with
`NEXT_PUBLIC_API_BASE` in `.env.local` for a different setup, e.g.:

```
NEXT_PUBLIC_API_BASE=http://localhost:8080
```

## Structure

| Path | Purpose |
|---|---|
| `lib/api.ts` | typed fetch client for every backend endpoint; the one place that knows the API shape |
| `lib/auth-context.tsx` | React context holding the signed-in user; persists only the bearer token (in `localStorage`), re-derives the user via `GET /api/auth/me` on reload |
| `app/page.tsx` | traffic director: routes to `/setup`, `/login`, or `/dashboard` |
| `app/setup` | first-run business + owner account creation |
| `app/login` | staff/owner sign-in |
| `app/dashboard` | landing screen after sign-in |
| `app/products` | product catalog: list, create (assigns a QR code), view stock |

Sales (QR-first billing) and purchases (manual + OCR-assisted) screens are
not built yet — those are V0.1/V0.2 per the roadmap.

## Notes

- This is Next.js 16 (Turbopack, App Router). `params`/`searchParams` are
  promises; none of the current pages use dynamic segments yet, so this
  hasn't come up, but it will for e.g. `/products/[id]`.
- No CSS framework — plain CSS in `app/globals.css` using custom properties
  with a `prefers-color-scheme: dark` override, kept intentionally simple
  for a V0 scaffold.
