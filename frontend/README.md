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
| `lib/api.ts` | typed fetch client for every backend endpoint; the one place that knows the API shape. Connects dynamically based on origin for LAN setups. |
| `lib/auth-context.tsx` | React context holding the signed-in user |
| `components/layout/Shell.tsx` | Application shell with navigation sidebar and header. Responsive to mobile sizes. |
| `components/layout/ConnectionStatus.tsx` | Live health-check polling component indicating if local server is reachable |
| `components/ui/` | Reusable state UI components (EmptyState, ErrorState, LoadingState) |
| `app/(app)` | The authenticated/main area wrapped by the Shell |
| `app/(app)/dashboard`, `sales`, `purchases`, `products`, `inventory`, `reports`, `settings` | Shell placeholders for core application functionality |
| `app/page.tsx` | traffic director: routes to `/setup`, `/login`, or `/dashboard` |
| `app/setup` | first-run business + owner account creation |
| `app/login` | staff/owner sign-in |

Sales, Purchases, Inventory, Reports, and Settings screens are currently placeholders awaiting implementation in future phases.

## Notes

- This is Next.js 16 (Turbopack, App Router). `params`/`searchParams` are
  promises; none of the current pages use dynamic segments yet, so this
  hasn't come up, but it will for e.g. `/products/[id]`.
- No CSS framework — plain CSS in `app/globals.css` using custom properties
  with a `prefers-color-scheme: dark` override, kept intentionally simple
  for a V0 scaffold.
