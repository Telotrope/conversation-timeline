# Report "diff" links that open VS Code's side-by-side comparison

**Date:** 2026-10-07
**Status:** proposed, waiting for approval

## 1. Goal

In the rendered preview of
[the load-only implementation report](../analysis/2026-10-07-load-only-what-the-page-shows-implementation-report.md),
clicking a row's "diff" link opens the same side-by-side comparison tab that
the Source Control panel opens, for that one file, between the commit before
the work (`3ae3c16`) and the commit at its end (`b2dc683`). The link must be
short enough that the report's rows stay readable as text too.

## 2. What was established before this plan

Read in the installed code-server (`/usr/lib/code-server/lib/vscode/`), not yet
seen working end to end:

- **Rendered preview link clicks.** In
  `extensions/markdown-language-features/media/index.js`, links starting
  `http:`, `https:`, `mailto:`, `vscode:` or `vscode-insiders:` are handled by
  the browser; every other link is passed to the Markdown extension, which (in
  `dist/extension.js`, `openDocumentLink`) opens any link with a scheme other
  than `file` through VS Code's `vscode.open` command.
- **`vscode.open` refuses command links.** In
  `out/vs/workbench/workbench.web.main.internal.js`, the `_workbench.open`
  handler returns without doing anything when the address starts `command:`.
  This is why a `command:vscode.diff?…` link works on Ctrl+click in the text
  editor (tested by the user, 2026-10-07) but cannot work in the preview.
- **`code-oss:` links reach extensions.** For any other address,
  `vscode.open` hands it to VS Code's opener. In the same file, an opener
  (minified class `Kuo`) takes addresses whose scheme equals the product's
  `urlProtocol` and passes them to the URL service with `trusted: true`. This
  code-server's `product.json` sets `urlProtocol` to `code-oss`. The URL
  service delivers `code-oss://<publisher>.<extension>/…` to that extension's
  URI handler (a function an extension registers to receive such links).
- **No installed extension opens a comparison from a link.** The only
  built-in URI handlers are Git (`/clone` only, read in
  `extensions/git/dist/main.js`), GitHub and Microsoft sign-in, and Copilot.
- **Source Control's address for an old version.** The Git extension's
  `toGitUri` (minified `je` in `extensions/git/dist/main.js`) builds
  `git:<absolute path>?{"path":"<absolute path>","ref":"<commit>"}`. The
  `command:` link the user tested used this format and opened the correct
  comparison.
- A web search found no existing extension or documented link format for this
  (sources listed in the 2026-10-07 conversation).

## 3. Design

### 3a. A small local extension, "diff-link"

New folder [devtools/vscode-diff-link/](../../devtools/vscode-diff-link/):

- `package.json` — publisher `telotrope`, name `diff-link`, activation on
  `onUri`, VS Code engine `^1.80.0`, no dependencies.
- `extension.js` (plain JavaScript, no build step, about 80 lines) — registers
  one URI handler with `vscode.window.registerUriHandler`.
- `uri_request.js` — parses and checks the link (§3b) into a request object,
  with no VS Code imports, so it is testable without VS Code.

Link format (what goes in the report):

```
code-oss://telotrope.diff-link/diff?path=backend/README.md&from=3ae3c16&to=b2dc683
```

The handler:

1. Parses the link once at the boundary (§3b). A bad link shows a VS Code
   error message naming the problem; nothing opens.
2. Finds the workspace folder that holds `path` (first open folder whose
   directory contains it; error message if none).
3. Checks with `git cat-file -e <commit>:<path>` (run with Node's
   `child_process.execFile`, arguments passed as a list, never through a
   shell) whether the file exists at each commit.
4. Opens:
   - both exist → `vscode.diff` with two `git:` addresses built exactly like
     `toGitUri`, titled `backend/README.md (3ae3c16 ↔ b2dc683)`;
   - only the new one exists (file added by the work) → `vscode.diff` with an
     empty left side, so the whole file shows as added — the same look
     Source Control uses for an added file;
   - only the old one exists (file removed) → the mirror image;
   - neither → error message naming both commits.
5. Every failure (git not found, git error, `vscode.diff` rejecting) ends in
   `vscode.window.showErrorMessage` with the error text, and is written to an
   output channel "Diff link".

The empty side is a read-only empty document provided by the
extension's own `diff-link-empty:` content provider, so no blank editor tab is
left behind.

### 3b. Parsing at the boundary

The link comes from a document, so it is untrusted text. `uri_request.js`
turns it into `{ path: RepoPath, from: CommitId, to: CommitId }`:

- `RepoPath` — relative, `/`-separated, no empty, `.` or `..` segments, no
  NUL or control characters. Rejecting `..` keeps a link from reaching files
  outside the workspace folder.
- `CommitId` — 7 to 40 hexadecimal characters. Anything else (branch names,
  `HEAD~1`, option-looking text such as `--output`) is rejected, so nothing a
  link carries can be read by git as an option.
- Unknown query fields are rejected, not ignored, so a mistyped field name
  fails loudly.

Trade-off chosen: a link with one bad field is rejected whole. The report's
links are generated (§3d), so a bad one is a bug to fix, not something to
open partly.

### 3c. Installing it

New script [scripts/install-diff-link.sh](../../scripts/install-diff-link.sh):
packages the folder with `npx @vscode/vsce package` (MIT license) and installs
the result with `code-server --install-extension`. After installing, the
browser tab is reloaded once.

### 3d. Changing the report's links

A one-off Python run (not kept in the repo; the report is Claude-written, not
produced by product code) rewrites every row whose "diff" link is a GitHub
compare address with a `#diff-…` anchor, taking the path from the same row's
file link. The GitHub compare address is replaced, not kept beside it (open
question Q1). Row count before and after is printed and must match the
report's stated 246 files plus any test-change rows that carry diff links.

**Step order:** the extension is installed and the user clicks row F2 in the
rendered preview *before* the other rows are rewritten. If the click fails,
the rewrite does not happen and the failure is reported.

## 4. Tests

- **Unit tests, Node's built-in `node:test`** (no new dependency), in
  `devtools/vscode-diff-link/test/`, through the extension's public entry
  point: `activate(context)` is called with a stand-in for the `vscode`
  module (substituted through Node's module loader), the registered handler
  is given links, and the test checks which `vscode.diff` call or error
  message results. Cases: both sides exist; added file; removed file;
  neither; each parse rejection in §3b; path outside every workspace folder;
  git failing to start; `vscode.diff` rejecting. The git checks run against a
  temporary real git repository made in the test, not a stand-in.
- **100% line coverage** measured with `node --test
  --experimental-test-coverage` (Node 18.19 on this machine supports it).
- **Lint for silent swallows:** every `catch` in the two files ends in an
  error message or a thrown error (checked by reading; the Python ratchet test
  does not scan JavaScript).
- **End to end:** the user clicks F2 in the rendered preview (§3d step
  order). Until then the status is "code-level only, end-to-end TBD".

## 5. Not covered

- Desktop VS Code uses `vscode://`, not `code-oss://`; these links only work
  in this code-server.
- Files renamed by the work show as one removed and one added, because rows
  are per path.

## 6. Open questions

- **Q1.** Replace the GitHub link, or keep both? Recommended: replace —
  the user reads the report in code-server, and two links per row is what
  made the test row unreadable.

## Self-critique log

### C1 [OPEN]: The route through the preview is read from minified code, not yet seen working
The chain preview → `vscode.open` → `code-oss` opener → extension is inferred
from reading minified source. **Mitigation in plan:** the F2 click test comes
before any report rewrite ([§3d, line 127](docs/plans/2026-10-07-report-diff-links-in-vs-code.md#L127)).
**Open:** if the click does nothing, check the "Diff link" output channel and
the browser console, and come back with a revised plan.

### C2 [OPEN]: VS Code may ask "Allow 'diff-link' to open this URI?" on first click
The opener passes `trusted: true`, which I expect skips the question, but I
have not read the URL service's confirmation code. **Open:** if the question
appears, the user can tick "don't ask again"; revisit if it appears every
time.

### C3 [RESOLVED]: An empty left side for added files could leave a blank tab
Original concern: using `untitled:` for the empty side opens an editable
empty buffer. **Resolution:** the extension provides its own read-only empty
document ([§3a, line 89](docs/plans/2026-10-07-report-diff-links-in-vs-code.md#L89)).

### C4 [RESOLVED]: Link text from a document could make git read an option or another file
Original concern: `path` or a commit beginning `-`, or containing `..`, would
be passed to git. **Resolution:** strict parsing of both fields and `execFile`
with an argument list ([§3b, line 93](docs/plans/2026-10-07-report-diff-links-in-vs-code.md#L93)).

### C5 [RESOLVED]: Reuse before new code
Original concern: an existing extension might already do this. **Resolution:**
the installed URI handlers were read and a web search done; none opens a
comparison ([§2, line 38](docs/plans/2026-10-07-report-diff-links-in-vs-code.md#L38)).
The `git:` address format is reused from the Git extension's own `toGitUri`
rather than invented.
