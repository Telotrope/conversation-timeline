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

### Testing this end to end without doing it by hand

[e2e/](e2e/README.md) has a real, repeatable Playwright suite that drives an actual browser through
this exact flow — `cd e2e && npm install && npm test`.
