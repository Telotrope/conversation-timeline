# Report "diff" links that open VS Code's side-by-side comparison, and line links that work in the preview

**Date:** 2026-10-07
**Status:** proposed, waiting for approval

## 1. Goal

In the rendered preview of
[the load-only implementation report](../analysis/2026-10-07-load-only-what-the-page-shows-implementation-report.md),
clicking a row's "diff" link opens the same side-by-side comparison tab that
the Source Control panel opens, for that one file, between the commit before
the work (`3ae3c16`) and the commit at its end (`b2dc683`). The link must be
short enough that the report's rows stay readable as text too.

Second goal (added 2026-10-07 after the user asked): a link to a line of a
Markdown document — from this chat panel, or from one document's preview to
another — ends with the document shown in the rendered preview, scrolled to
that line, instead of the preview opening at the top.

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

Why line links fail in the preview, read in
`extensions/markdown-language-features/` (also not yet seen working):

- **The preview only jumps to headings.** A link such as `report.md#L984`
  reaches the preview as the "fragment" `L984`. The preview's script
  (`media/index.js`, minified function `Ae`) looks for a page element whose
  `id` equals the fragment. Only headings have ids, so `L984` matches nothing
  and the preview stays at the top. The text editor, by contrast, reads
  `L984` as a line number (`dist/extension.js`, minified function `$G`).
- **Every block in the preview knows its source line.** The preview marks
  each paragraph, heading, list item and table row with `data-line` (its
  first source line, counted from 0) and the class `code-line`
  (`dist/extension.js`, rule `source_map_data_attribute`). A script can
  therefore find the element for any line.
- **Extensions can add a script to every preview** through the
  `markdown.previewScripts` contribution, which the Markdown extension loads
  into the preview page.
- **`markdown.showPreview` keeps a fragment.** Given a document address with
  a fragment, it opens the preview with that fragment (`openDynamicPreview`:
  `t.fragment ? new J_(t.fragment) : …`). Given none, it starts at the text
  editor's *top visible* line (`#p`, `ep`), not the line the cursor is on —
  so a line revealed in the middle of the screen ends up half a screen below
  where the preview starts.
- **This chat panel always opens files as text.** The Claude Code extension
  opens a file link with `vscode.window.showTextDocument` and then selects the
  linked line; any other link goes to the browser (read in
  `~/.local/share/code-server/extensions/anthropic.claude-code-2.1.292-linux-x64/extension.js`,
  `openFile` and `openURL`). Another extension cannot change that, so the
  switch to the preview has to happen after the text editor opens (§3f).

## 3. Design

### 3a. A small local extension, "doc-links"

The extension now does three things (comparison links, line links in the
preview, switching to the preview), so it is named "doc-links" rather than
"diff-link". New folder [devtools/vscode-doc-links/](../../devtools/vscode-doc-links/):

- `package.json` — publisher `telotrope`, name `doc-links`, activation on
  `onUri` and `onLanguage:markdown`, VS Code engine `^1.80.0`; contributes the
  preview script (§3e) and the command and editor-title button (§3f). Only
  development dependency: `jsdom` (MIT) for testing the preview script.
- `extension.js` (plain JavaScript, no build step) — `activate` wires the
  pieces below; each piece is its own file.
- `diff_link.js` — the URI handler for comparison links.
- `uri_request.js` — parses and checks the link (§3b) into a request object,
  with no VS Code imports, so it is testable without VS Code.
- `media/line_fragment.js` — the preview script (§3e).
- `preview_switch.js` — the command and the automatic switch (§3f).

Link format (what goes in the report):

```
code-oss://telotrope.doc-links/diff?path=backend/README.md&from=3ae3c16&to=b2dc683
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
   output channel "Doc links".

The empty side is a read-only empty document provided by the
extension's own `doc-links-empty:` content provider, so no blank editor tab is
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

New script [scripts/install-doc-links.sh](../../scripts/install-doc-links.sh):
packages the folder with `npx @vscode/vsce package` (MIT license) and installs
the result with `code-server --install-extension`. After installing, the
browser tab is reloaded once.

### 3d. Changing the report's links

A one-off Python run (not kept in the repo; the report is Claude-written, not
produced by product code) rewrites every row whose "diff" link is a GitHub
compare address with a `#diff-…` anchor, taking the path from the same row's
file link. The GitHub compare address is replaced, not kept beside it (decided by the
user 2026-10-07, Q1). Row count before and after is printed and must match the
report's stated 246 files plus any test-change rows that carry diff links.

**Step order:** the extension is installed and the user clicks row F2 in the
rendered preview *before* the other rows are rewritten. If the click fails,
the rewrite does not happen and the failure is reported.

### 3e. Line links scroll the preview to the line

`media/line_fragment.js`, loaded into every preview through
`markdown.previewScripts`:

1. Reads the preview's settings from the page element
   `vscode-markdown-preview-data` (attribute `data-settings`, the same place
   the preview's own script reads them) and takes `fragment`.
2. If the fragment is a line reference — `L984`, `984`, or a range
   `L984-L990`, the same forms the text editor accepts (`$G`) — it waits
   until the rendered content is on the page (a `MutationObserver`, given up
   after 2 seconds), then picks the `code-line` element with the largest
   `data-line` not past the target line (the block that contains the line).
3. Scrolls that element into view near the top of the window and marks it
   with the preview's own `code-active-line` class, which the preview already
   draws as a bar beside the active line.
4. Any other fragment (a heading) is left to the preview's own code.

This covers links from one document's preview to another (the preview passes
the fragment through when it opens a linked Markdown file in the preview) and
the switch in §3f. A line inside a multi-line paragraph scrolls to the start
of that paragraph; a line inside a code block scrolls to the start of the
block.

### 3f. Getting from a chat link to the preview, at the line

Two ways, both opening the preview with `markdown.showPreview` on the
document's address plus the fragment `L<line>`, so §3e scrolls it:

- **A button and command, "Open Preview at Cursor".** Shown in the title bar
  of Markdown text editors. Opens the preview at the cursor's line and closes
  the text tab (unless it has unsaved changes, in which case it stays open).
- **Automatic switch** for Markdown files under the workspace's `docs/`
  folder. When a new text tab opens for such a file and no preview of it is
  already open, the extension waits up to half a second for the opener to
  select a line (the chat panel selects the linked line right after
  opening), then does what the button does. With no selection, it uses the
  line at the top of the screen.

The automatic switch is skipped when a preview of the same file is already
open. That rule is what lets the preview's "Open Source" button show the text
without being switched straight back, because "Open Source" is only
reachable from an open preview. Its cost: a second chat link into a document
whose preview is already open opens the text; the button then takes one
click.

## 4. Tests

- **Unit tests, Node's built-in `node:test`** (no new dependency), in
  `devtools/vscode-doc-links/test/`, through the extension's public entry
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
- **Preview script tests** with `jsdom` (MIT): a page built like the preview's
  (settings element plus `code-line` elements with `data-line`) is loaded with
  the script; the test checks which element is scrolled to and marked, for a
  line on a block's first line, a line inside a paragraph, a range, a
  heading fragment (untouched), no fragment, and content arriving after the
  script runs.
- **Command and automatic switch tests** through `activate` with the same
  `vscode` stand-in: button with and without unsaved changes; new `docs/` tab
  with a selection, without one, with a preview already open; a file outside
  `docs/`; a non-Markdown file.
- **End to end:** the user clicks F2 in the rendered preview (§3d step
  order), and clicks a chat link to a report line. Until then the status is
  "code-level only, end-to-end TBD".

## 5. Not covered

- Desktop VS Code uses `vscode://`, not `code-oss://`; these links only work
  in this code-server.
- Files renamed by the work show as one removed and one added, because rows
  are per path.
- Plain file links from the chat panel (no line) still open as text first;
  the automatic switch then moves them to the preview, so the text tab
  flashes briefly.

## 6. Open questions

- **Q1 [DECIDED 2026-10-07].** Replace the GitHub link, or keep both?
  The user: replace every row's GitHub link ("they're useless anyway").
- **Q2.** Automatic switch (§3f) on, or only the button? Recommended: both —
  the automatic switch covers the common case (chat link into a report), the
  button covers a document whose preview is already open.

## Self-critique log

### C1 [OPEN]: The route through the preview is read from minified code, not yet seen working
The chain preview → `vscode.open` → `code-oss` opener → extension is inferred
from reading minified source. **Mitigation in plan:** the F2 click test comes
before any report rewrite ([§3d, line 169](docs/plans/2026-10-07-report-diff-links-in-vs-code.md#L169)).
**Open:** if the click does nothing, check the "Doc links" output channel and
the browser console, and come back with a revised plan.

### C2 [OPEN]: VS Code may ask "Allow 'doc-links' to open this URI?" on first click
The opener passes `trusted: true`, which I expect skips the question, but I
have not read the URL service's confirmation code. **Open:** if the question
appears, the user can tick "don't ask again"; revisit if it appears every
time.

### C3 [RESOLVED]: An empty left side for added files could leave a blank tab
Original concern: using `untitled:` for the empty side opens an editable
empty buffer. **Resolution:** the extension provides its own read-only empty
document ([§3a, line 131](docs/plans/2026-10-07-report-diff-links-in-vs-code.md#L131)).

### C4 [RESOLVED]: Link text from a document could make git read an option or another file
Original concern: `path` or a commit beginning `-`, or containing `..`, would
be passed to git. **Resolution:** strict parsing of both fields and `execFile`
with an argument list ([§3b, line 135](docs/plans/2026-10-07-report-diff-links-in-vs-code.md#L135)).

### C6 [RESOLVED]: An automatic switch could flip "Open Source" straight back to the preview
Original concern: switching every Markdown text tab to the preview would make
the preview's "Open Source" button useless. **Resolution:** the switch is
skipped when a preview of that file is already open, and limited to `docs/`
([§3f, line 205](docs/plans/2026-10-07-report-diff-links-in-vs-code.md#L205)).

### C7 [OPEN]: The chat panel's selection timing is read from minified code
The automatic switch relies on the chat panel selecting the linked line soon
after the text tab opens. **Mitigation in plan:** wait up to half a second,
then fall back to the top visible line. **Open:** if the preview lands on the
wrong line in the end-to-end check, log the selection events to the "Doc
links" output channel and revise.

### C8 [OPEN]: Scrolling done by a second script could fight the preview's own scrolling
The preview syncs scroll position with the text editor when both are visible.
**Mitigation in plan:** the script scrolls once, only when a line fragment is
present, and the preview's own code does nothing for a fragment it cannot
match. **Open:** if the preview jumps after landing, check in the end-to-end
run with the text editor visible beside it.

### C5 [RESOLVED]: Reuse before new code
Original concern: an existing extension might already do this. **Resolution:**
the installed URI handlers were read and a web search done; none opens a
comparison ([§2, line 43](docs/plans/2026-10-07-report-diff-links-in-vs-code.md#L43)).
The `git:` address format is reused from the Git extension's own `toGitUri`
rather than invented. The line scroll reuses the preview's own `data-line`
marks and `code-active-line` style, and the fragment forms reuse the text
editor's `L984` convention.
