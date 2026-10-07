# Report "diff" links that open VS Code's side-by-side comparison, and line links that work in the preview

**Date:** 2026-10-07
**Status:** revision 2 (2026-10-07), waiting for approval. Revision 1 was
approved and built; its line links and switch to the preview work (user,
2026-10-07), but its comparison links do nothing in the preview (C1). This
revision changes only how a comparison link reaches the extension (route A,
chosen by the user 2026-10-07): §2, §3a, §3g, §4 and C1, C9–C11.

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

- **Rendered preview link clicks — corrected in revision 2.** Revision 1
  read `extensions/markdown-language-features/media/index.js` as passing
  every link without a web scheme to the Markdown extension. It does not: it
  passes a link to the Markdown extension only when the link has **no scheme
  at all** (a relative path). A link with any scheme (`code-oss:`, `https:`,
  …) is left to the webview, the sandboxed page the preview runs in.
- **What the webview opens.** The webview host opens a clicked link only if
  `isSupportedLink` allows it (`workbench.web.main.internal.js`): `http`,
  `https`, `mailto`, `vscode`, `vscode-insider`; `command` only when the
  webview enables command links (the preview does not); and the product's own
  scheme (`code-oss`) **only when not running in a browser**. code-server runs
  in a browser, so revision 1's `code-oss:` links were dropped without a
  message — the F2 test (user, 2026-10-07).
- **How a web link opens.** For an `https:` link the webview calls VS Code's
  opener with `fromUserGesture`, `allowContributedOpeners` and
  `fromWorkspace` set. The opener first runs its checks: the link-protection
  check (the "open external website?" question) is skipped when the link
  comes `fromWorkspace`, the workspace is trusted, and
  `workbench.trustedDomains.promptInTrustedWorkspace` is off (its default).
  It then offers the link to openers that extensions registered with
  `vscode.window.registerExternalUriOpener`, before falling back to the
  browser. An extension opener that answers "Preferred"
  (`ExternalUriOpenerPriority.Preferred`, 3) is used without asking which
  opener to use.
- **That hook is a "proposed" VS Code feature.** The extension API
  `registerExternalUriOpener(id, opener, { schemes, label })` exists in this
  build's extension host but is allowed only for extensions that declare
  `"enabledApiProposals": ["externalUriOpener"]` and that code-server is
  started to allow, through its `enable-proposed-api` option (a list of
  extension IDs; code-server's `config.yaml` accepts a YAML list for it, read
  in `/usr/lib/code-server/out/node/cli.js`). Only `http` and `https` are
  accepted. The opener object needs `canOpenExternalUri(uri, token)` and
  `openExternalUri(resolvedUri, { sourceUri }, token)`. If
  `openExternalUri` fails, VS Code shows a notification offering to open the
  link in the browser instead.
- **`vscode.open` refuses command links.** In
  `out/vs/workbench/workbench.web.main.internal.js`, the `_workbench.open`
  handler returns without doing anything when the address starts `command:`.
  This is why a `command:vscode.diff?…` link works on Ctrl+click in the text
  editor (tested by the user, 2026-10-07) but cannot work in the preview.
- **`code-oss:` links reach extensions — but not from a webview in a
  browser.** `vscode.open` hands other addresses to VS Code's opener, where an
  opener (minified class `Kuo`) passes `code-oss://<publisher>.<extension>/…`
  to that extension's URI handler. That route is real, but the preview never
  reaches it (see the corrected bullets above), so revision 2 drops it.
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
"diff-link".

**Location (user decision 2026-10-07): outside the timeline repository**, as
its own git repository at `~/workspace/vscode-doc-links/` (a sibling of
`conversation-timeline/`). It is a reading aid for code-server, not part of
the timeline product. Only the report rewrite (§3d) and this plan change the
timeline repository. Files in the new repository:

- `package.json` — publisher `telotrope`, name `doc-links`, activation on
  `onStartupFinished` (when code-server finishes loading; changed
  from `onLanguage:markdown` during coding and confirmed by the user
  2026-10-07, because an extension started by the first Markdown file opening
  would start too late to switch that file's tab), VS Code engine `^1.80.0`;
  `enabledApiProposals: ["externalUriOpener"]` (revision 2, §3g);
  contributes the
  preview script (§3e) and the command and editor-title button (§3f). Only
  development dependency: `jsdom` (MIT) for testing the preview script.
- `extension.js` (plain JavaScript, no build step) — `activate` wires the
  pieces below; each piece is its own file.
- `diff_link.js` — the link opener for comparison links (revision 2; was a
  URI handler).
- `uri_request.js` — parses and checks the link (§3b) into a request object,
  with no VS Code imports, so it is testable without VS Code.
- `media/line_fragment.js` — the preview script (§3e).
- `preview_switch.js` — the command and the automatic switch (§3f).

Link format (what goes in the report), revision 2:

```
https://doc-links.invalid/diff?path=backend/README.md&from=3ae3c16&to=b2dc683
```

`.invalid` is a top-level name reserved never to exist on the internet, so
the address can only ever mean this extension; if the extension is not
running, the browser opens a tab that fails to load instead of reaching a real
site (C11).

The opener registers for `https` links, answers "Preferred" for links whose
host is `doc-links.invalid` and "None" for every other link, so all other web
links open as before. When it is given a link, it:

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

If VS Code refuses to register the opener (code-server not started with the
`enable-proposed-api` option, §3g), the extension shows one error message
saying comparison links are off and why, writes it to the "Doc links"
output, and starts the rest — line links and the switch to the preview —
normally (C9).

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

Script `install.sh` at the root of `~/workspace/vscode-doc-links/`:
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

### 3g. code-server configuration (revision 2)

One line added to `~/.config/code-server/config.yaml` (the file's other
settings untouched):

```yaml
enable-proposed-api: [telotrope.doc-links]
```

code-server reads it only at start, so it is restarted once with
`sudo systemctl restart code-server@molinemc` (it runs as that system
service; the command needs the user's password). **A restart stops every
Claude session running inside code-server, this one included.** Order agreed
with the user 2026-10-07: Claude adds the line; the user restarts when the
other sessions are idle; the user reopens this conversation from the Claude
panel's session history; Claude then writes the code. Checked after the
restart, before coding: the extension host log shows code-server started
with the option (the extension logs whether the opener registered, §3a).

## 4. Tests

- **Unit tests, Node's built-in `node:test`** (no new dependency), in
  `~/workspace/vscode-doc-links/test/`, through the extension's public entry
  point: `activate(context)` is called with a stand-in for the `vscode`
  module (substituted through Node's module loader), the registered opener
  is given links, and the test checks which `vscode.diff` call or error
  message results. Revision 2 changes the comparison-link tests in
  `test/diff_link.test.js` (committed in revision 1) to give links to the
  opener instead of the URI handler, with `https://doc-links.invalid/…`
  links; approving this revision approves that test change. Added cases: the
  opener answers "Preferred" for its host and "None" for other hosts and for
  `http`; registration refused (the proposal is off) shows the one message
  and leaves the preview switch working. Cases: both sides exist; added file; removed file;
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

- Comparison links need code-server started with `enable-proposed-api`
  (§3g). A code-server upgrade could change or remove the proposed hook (C10).
- Files renamed by the work show as one removed and one added, because rows
  are per path.
- Plain file links from the chat panel (no line) still open as text first;
  the automatic switch then moves them to the preview, so the text tab
  flashes briefly.

## 6. Open questions

- **Q1 [DECIDED 2026-10-07].** Replace the GitHub link, or keep both?
  The user: replace every row's GitHub link ("they're useless anyway").
- **Q2 [DECIDED 2026-10-07].** Automatic switch (§3f) on, or only the
  button? Recommended both; the user approved the plan with that
  recommendation ("Start coding now").
- **Q3 [DECIDED 2026-10-07].** Where does the extension live? The user:
  outside the timeline repository (§3a).

## Self-critique log

### C1 [OPEN]: The route through the preview is read from minified code, not yet seen working
The chain preview → `vscode.open` → `code-oss` opener → extension is inferred
from reading minified source. **Mitigation in plan:** the F2 click test comes
before any report rewrite ([§3d, line 225](docs/plans/2026-10-07-report-diff-links-in-vs-code.md#L225)).
**Result of the F2 click (user, 2026-10-07): nothing happened; the "Doc
links" output stayed empty.** Cause, read in the installed code-server after
the failure:
- §2's first bullet was misread. The preview's click handler
  (`media/index.js`) passes a link to the Markdown extension only when it has
  *no* scheme; a `code-oss:` link is left to the webview (the sandboxed page
  the preview runs in).
- The webview host opens a clicked link only if `isSupportedLink` allows it
  (`workbench.web.main.internal.js`): `http`, `https`, `mailto`, `vscode`,
  `vscode-insider`, `command` when the webview enables command links (the
  preview does not), or the product's own scheme (`code-oss`) **only when not
  running in a browser**. code-server runs in a browser, so the click is
  dropped without a message.
The line links and the switch to the preview (§3e, §3f) work (user,
2026-10-07). **Mitigation in revision 2:** comparison links become `https:`
links caught by an extension link opener (route A, chosen by the user,
[§3a, line 158](docs/plans/2026-10-07-report-diff-links-in-vs-code.md#L158)).
**Open:** the same F2 click test, after the restart in §3g; if it fails, the
"Doc links" output and the extension host log are read before anything else
is changed.

### C2 [RESOLVED]: VS Code may ask "Allow 'doc-links' to open this URI?" on first click
Original concern: the URI-handler route might ask before handing a link to
the extension. **Resolution:** moot in revision 2, which no longer uses a URI
handler; the opener route's only question, link protection, is skipped for
links clicked in a trusted workspace
([§2, line 42](docs/plans/2026-10-07-report-diff-links-in-vs-code.md#L42)).

### C3 [RESOLVED]: An empty left side for added files could leave a blank tab
Original concern: using `untitled:` for the empty side opens an editable
empty buffer. **Resolution:** the extension provides its own read-only empty
document ([§3a, line 181](docs/plans/2026-10-07-report-diff-links-in-vs-code.md#L181)).

### C4 [RESOLVED]: Link text from a document could make git read an option or another file
Original concern: `path` or a commit beginning `-`, or containing `..`, would
be passed to git. **Resolution:** strict parsing of both fields and `execFile`
with an argument list ([§3b, line 191](docs/plans/2026-10-07-report-diff-links-in-vs-code.md#L191)).

### C6 [RESOLVED]: An automatic switch could flip "Open Source" straight back to the preview
Original concern: switching every Markdown text tab to the preview would make
the preview's "Open Source" button useless. **Resolution:** the switch is
skipped when a preview of that file is already open, and limited to `docs/`
([§3f, line 261](docs/plans/2026-10-07-report-diff-links-in-vs-code.md#L261)).

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

### C9 [RESOLVED]: If the proposed hook is off, the whole extension could fail to start
Original concern: `registerExternalUriOpener` throws when the proposal is not
enabled, which would stop `activate` and take the working line links down
with it. **Resolution:** the registration failure is caught, reported once,
and the rest of the extension starts
([§3a, line 185](docs/plans/2026-10-07-report-diff-links-in-vs-code.md#L185)).

### C10 [OPEN]: A proposed hook can change in any code-server upgrade
**Mitigation in plan:** the failure path in C9 makes a removed or renamed
hook visible (one error message) instead of silent. **Open:** after each
code-server upgrade, if that message appears, read the new build's
extension-host API and revise.

### C11 [OPEN]: Without the extension, a comparison link opens a dead browser tab
If the extension is not installed or not running, the `.invalid` link opens a
browser tab that fails to load. **Mitigation in plan:** `.invalid` can never
reach a real site. **Open:** acceptable unless the reports are read
somewhere without the extension (e.g. on GitHub); revisit then.

### C5 [RESOLVED]: Reuse before new code
Original concern: an existing extension might already do this. **Resolution:**
the installed URI handlers were read and a web search done; none opens a
comparison ([§2, line 74](docs/plans/2026-10-07-report-diff-links-in-vs-code.md#L74)).
The `git:` address format is reused from the Git extension's own `toGitUri`
rather than invented. The line scroll reuses the preview's own `data-line`
marks and `code-active-line` style, and the fragment forms reuse the text
editor's `L984` convention.
