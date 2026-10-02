# Deferred problems

**Status:** open. No fix is designed yet, except item 5's proposal (not approved). Each problem
gets its own plan, or a section here, before any code changes.

These came from the "Known defects" section of
[2026-09-28-frontend-quality-of-life.md](completed/2026-09-28-frontend-quality-of-life.md), moved
here on 2026-10-02 so that plan could be filed as completed. The numbered text is unchanged
from that plan. The "State on 2026-10-02" note under each item records what I read in the code
on the day it moved.

## Problems — fix before shipping

Items 1 and 2 were found while verifying that plan's Phase 3 (deleting the page's own detection
code). Phase 3 didn't cause either one, and the user scheduled them behind the quality-of-life
phases.

1. **Conversation order is non-deterministic across backend restarts.** `list_for_user` in
   [backend/timeline-storage/src/memory/conversations.rs](../../backend/timeline-storage/src/memory/conversations.rs)
   iterates a `HashMap` and collects without sorting, and Rust seeds its hasher randomly per
   process. Observed directly: three runs of the *same* unmodified page produced three different
   conversation orders. User-visible as the conversation list and the per-conversation colors
   reshuffling between sessions, and it makes any test that says "the first conversation"
   order-dependent — one did, and had to be rewritten. Fix: sort the returned summaries by a stable
   key. Note `export.rs` already sorts `upload_ids` but never sorts `summaries`.

   *State on 2026-10-02:* unfixed. I read
   [conversations.rs:36-47](../../backend/timeline-storage/src/memory/conversations.rs#L36-L47);
   it still collects without sorting. The only sort of summaries is in
   [detect.rs:82](../../backend/timeline-api/src/routes/detect.rs#L82), which the export doesn't
   use. Not checked: whether the DynamoDB version of `list_for_user` returns a stable order.

2. **Intermittent "Failed to fetch" from the page to the backend, cause unknown.** Several capture
   runs failed with the page reporting it could not reach `http://127.0.0.1:3000` while the backend
   was demonstrably listening and answering `curl`. It affected the pre-deletion build equally, so
   it is not a regression. Serving over `http` instead of `file://` did not eliminate it; explicitly
   waiting for reachability before driving the browser did. **Not root-caused** — worked around, not
   explained. Worth isolating before shipping, since a user hitting this sees only "Is the backend
   running?" when it is.

   *State on 2026-10-02:* still not root-caused. One possible link, which nobody has tested:
   [2026-10-01-browser-tests-own-server.md](completed/2026-10-01-browser-tests-own-server.md)
   found tests talking to an old server left on port 3000. Whether that explains these failures
   is unknown.

3. **Friction ranking can only be sorted by % flagged.** Added 2026-09-30 at the user's request.
   It should sort by any column, and needs a new start-date column (a session's first message; a
   conversation's earliest session). Today `computeFrictionAnalysis` in
   [frontend/core/analyses.js](../../frontend/core/analyses.js) sorts once by percentage, and
   `renderFrictionResult` in [frontend/ui/views/analytics.js](../../frontend/ui/views/analytics.js)
   draws fixed headers.

   *State on 2026-10-02:* unfixed. [analyses.js:78](../../frontend/core/analyses.js#L78) sorts by
   percentage only.

4. **Check the review table's hover text once AI classification exists.** Added 2026-09-30 at
   the user's request. Hovering a checkbox whose value is automatic says where it came from
   ("automatic tag from keyword/sentiment heuristic"). Today only the scan produces automatic
   tags; when AI classification on Bedrock arrives (the migration plan's V3), confirm the hover
   names the right source. Marked with a TODO in
   [frontend/ui/views/review.js](../../frontend/ui/views/review.js).

   *State on 2026-10-02:* waiting on the migration plan's V3. The TODO is at
   [review.js:164](../../frontend/ui/views/review.js#L164).

5. **One status text is shared by three unrelated messages, and sits beside the download button.**
   Added 2026-10-02 at the user's request, found during deployment check D6. The Review tab has a
   single text spot, `saveStatus` ([timeline.html:815](../../timeline.html#L815)), in the bar with
   the "Download annotated conversations.json" button. Three things write to it, each replacing
   the last:
   - after loading, "Loaded N of your confirmed flags from the server."
     ([load-flow.js:50](../../frontend/ui/load-flow.js#L50));
   - after the download, "Downloaded conversations-with-flags.json …" or "No conversation data
     loaded to annotate." ([annotated-export.js:17](../../frontend/ui/annotated-export.js#L17),
     [line 61](../../frontend/ui/annotated-export.js#L61));
   - after every checkbox or Approve, the save's outcome, "Saved." or an error
     ([flag-edits.js:37](../../frontend/ui/flag-edits.js#L37)).

   So one Approve erases the load summary, and "Saved." reads as if it were about the download.
   It also doesn't say *which* row was saved. Separately, the row's "Reviewed" label changes before
   the server answers ([flag-edits.js:36-37](../../frontend/ui/flag-edits.js#L36-L37)), so a row
   can say "Reviewed" while its save failed, with the failure shown only in that distant text.

   **Proposed fix (not approved):**
   - *A save's outcome goes on its own row.* The row's status, from `reviewStatus`
     ([review.js:136](../../frontend/ui/views/review.js#L136)), reads "Saving…" while the request
     is out, "Reviewed" once the server answers, and "Not saved: <reason>" (the existing
     `saveMessage` wording) if it fails. The local change is kept either way, as today, so a failed
     row stays visibly unsaved until it is clicked again. This needs a per-row save state
     (`state.saveStates[id]`) beside `state.overrides`.
   - *The load summary moves next to the review count* (`reviewCount`,
     [timeline.html:811](../../timeline.html#L811)), where nothing else writes.
   - *The download's message stays beside the download button*, which is then the only thing
     writing there; `setSaveStatus` is renamed to say so (e.g. `setDownloadStatus`).
   - *Reuse check:* `reviewStatus` and `saveMessage` already exist and are extended, not
     duplicated; no new widget.
   - *Tests (browser, against the existing local backend):* approve a row → it shows "Saving…"
     then "Reviewed", and the load summary is still shown; a save the server refuses (a stale
     handle, 403) → that row shows "Not saved: …" and other rows are unchanged; downloading →
     the message appears beside the button only.
