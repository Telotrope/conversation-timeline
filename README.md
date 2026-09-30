# conversation-timeline

A tool for reviewing your exported Claude conversation history: session timing, flagged messages
(ALL-CAPS emphasis, criticism, anger), and a review workflow to confirm or correct those flags.
Originally a single self-contained `timeline.html` file; migrating to a Rust backend — see
[docs/plans/2026-09-09-rust-aws-backend-migration.md](docs/plans/2026-09-09-rust-aws-backend-migration.md)
for the full plan and [backend/README.md](backend/README.md) for the backend workspace itself.

## Running this locally

Two separate things need to be running at once — **the backend server** and **something serving
`timeline.html`**. Neither alone is enough.

### 1. Start the backend

```
cd backend
cargo run -p timeline-api
# -> listening on http://127.0.0.1:3000
```

Leave this running. See [backend/README.md](backend/README.md#prerequisites) if `cargo`/a C linker
aren't installed yet. This uses in-memory storage only — no AWS, no database, no container — but
that also means **restarting it deletes every upload, conversation, and flag**; see that README's
persistence note.

### 2. Serve `timeline.html` — don't just double-click it

```
# from the repository root, in a second terminal:
python3 -m http.server 8000
```

Then open **`http://localhost:8000/timeline.html`** in a browser — not `file:///path/to/timeline.html`.
Opening it as a local file does not reliably work: the page's `fetch()` calls to the backend
(`http://127.0.0.1:3000`) fail from a page loaded via `file://`, empirically confirmed, though the
exact browser security mechanism responsible hasn't been root-caused here. Any static file server
works, not just Python's — `npx serve`, `php -S localhost:8000`, etc. are equally fine; the only
requirement is that the page loads over `http://`/`https://`, not `file://`.

### 3. Use it

- Type any name into "Dev login name" (this is a stand-in for real login — there's no real account
  system yet, see [backend/README.md](backend/README.md#dev-only-signing-key-generated-not-checked-in)).
- Choose a `conversations.json` export and click Load.
- The file uploads to the backend, gets processed (deduplication, session splitting, flag
  detection), and comes back rendered — calendar, conversation list, and a review table where you
  can confirm or correct flags. Confirmed flags are saved back to the backend as you click, for as
  long as the server in step 1 keeps running.

### Running the backend and browser on different machines

The steps above assume the backend and the browser are the same machine, where the browser's own
`127.0.0.1:3000` reaches the backend directly. That's not true for a remote/cloud dev environment
(a devcontainer, a cloud IDE, a VM) reached from your own machine's browser through a
port-forwarding proxy — `127.0.0.1` in *your* browser always means *your* machine, never the
remote one, no matter what's actually listening on the far end.

In that case, once you know the URL your browser can actually reach the backend through (however
your environment forwards/proxies port 3000 — ask your environment's docs if unsure), open
`timeline.html` with it as a query parameter, once:

```
http://localhost:8000/timeline.html?api_base=https://your-forwarded-url/for/port/3000
```

This is remembered in `localStorage`, so you don't need to repeat it on later loads (from the same
browser) — see `resolveApiBase` in [frontend/infra/api-client.js](frontend/infra/api-client.js) for exactly what it does. If
requests still fail after this, check your browser's Network tab: some proxy setups gate access
behind their own login/session and will answer with their own error page instead of ever reaching
`timeline-api`. `timeline-api` itself only ever answers with one of a small, specific set of
messages — `"missing Authorization header"`, `"invalid or expired token"`, `"not found"`,
`"storage backend error"`, `"object store backend error"`, `"internal error"` (some as plain text,
some as `{"error": "..."}` JSON — see
[backend/timeline-api/src/error.rs](backend/timeline-api/src/error.rs) and
[backend/timeline-api/src/auth_extractor.rs](backend/timeline-api/src/auth_extractor.rs)) — so a
response with any other wording (a generic `"Unauthorized"`, an HTML login page, etc.) means
something in front of the backend answered instead of `timeline-api` itself, not a bug in this
tool.

### Testing this end to end without doing it by hand

[e2e/](e2e/README.md) has a real, repeatable Playwright suite that drives an actual browser through
this exact flow — `cd e2e && npm install && npm test`.
