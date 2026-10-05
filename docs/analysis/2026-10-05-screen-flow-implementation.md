# Screen flow: what was built, and how it measured

**Plan:** [docs/plans/2026-10-05-screen-flow.md](../plans/2026-10-05-screen-flow.md), steps 1–11 of
[§10c](../plans/2026-10-05-screen-flow.md#L943). All eleven steps were carried out on 2026-10-05.
Everything below was measured locally (in-memory stores, DynamoDB Local, the Cognito stand-in).
**Nothing has been deployed to AWS**, so nothing here is verified end to end on the real services.

## What was measured

| Suite | Before | After | Result |
|---|---|---|---|
| Rust (`cargo test --workspace`, with DynamoDB Local) | 413 | 465 | all pass |
| Page unit tests (`npm test` in [frontend/](../../frontend/)) | 144 | 164 | all pass |
| Browser tests (`npx playwright test` in [e2e/](../../e2e/)) | 78 | 102 | all pass, twice in a row with the final code |
| Script tests ([test-deploy-scripts.sh](../../scripts/test-deploy-scripts.sh), test-dev-up, test-activity-timeline, test-clean-build) | pass | pass | all pass |

Coverage of the new code:

- **Rust core** (`timeline-core`): the new modules ([labels.rs](../../backend/timeline-core/src/labels.rs),
  [conversation_metadata.rs](../../backend/timeline-core/src/conversation_metadata.rs)) and the changed
  [ports/uploads.rs](../../backend/timeline-core/src/ports/uploads.rs) are at 100%.
- **Rust API.** Every line of the new [routes/metadata.rs](../../backend/timeline-api/src/routes/metadata.rs)
  and [conversation_rebuild.rs](../../backend/timeline-api/src/conversation_rebuild.rs) runs except two
  lines marked as unreachable backstops in the code. The changed `processing.rs` still has uncovered
  lines, but those are older error paths (a file that isn't UTF-8), not new code.
- **Page core modules** (the new `conversation-metadata.js` and `upload-batch.js`, and the changed
  `blocks.js`): 100% of lines and branches in the unit tests.
- **Page modules, through the browser tests.** These are at 100% of lines:
  - `page-flow.js`, `describe-form.js`, `load-flow.js`;
  - `navigation/pages.js`, `widgets/loading-modal.js`;
  - `views/conversations.js`, `main.js`.

  The page's overall browser-test line coverage is 96.2%. The unrun lines that remain are older code.

## Deviations from the plan

Each was a choice made while coding, because the plan's wording ran into a rule or a fact it hadn't
accounted for.

1. **The metadata calls live in [api-client.js](../../frontend/infra/api-client.js).** The plan
   ([§9](../plans/2026-10-05-screen-flow.md)) named a separate `infra/metadata-client.js`. But the
   structure test forbids one API-layer file from importing another, and the new calls need
   `apiFetch`.
2. **The page modules are wired together by [main.js](../../frontend/main.js), not by importing each
   other.** The structure test forbids imports between the top-level page modules (`page-flow.js`,
   `load-flow.js`, `login-panel.js`, `describe-form.js`), so `main.js` hands each module the functions
   it needs from the others. The Upload page's handler stays in `load-flow.js` for the same reason.
3. **Which page you're on is recorded as an event, not a field.** The plan
   ([§9](../plans/2026-10-05-screen-flow.md)) said activity records would gain a `screen` field.
   Instead, each change of page is recorded as a `view` event with `via: 'page'`, and records keep
   their `tab` field. Adding a field would have broken the recorder tests' exact comparisons.
   Because `noteMainShown` stayed, the approved test change U1 wasn't needed.
4. **A file's conversation list on Describe has no per-conversation edit links.** The list is
   read-only, with a note pointing to the Conversations tab, which edits one conversation at a time.
   Links would have needed a return path from one Describe page to another.
5. **The warning for conversations with only some messages timed (Q19) isn't built.** The server
   rejects a message without a time (`created_at` is required by
   [model.rs](../../backend/timeline-core/src/model.rs)), so no upload can produce such a
   conversation today. Placing such conversations by their start and end is built and unit-tested.
6. **Matching conversations that have no id isn't built.** This was already deferred, as item 8 of
   [deferred-problems](../plans/2026-10-02-deferred-problems.md).
7. **Done confirms the guesses even when nothing was changed.** It saves every field it shows and
   marks them confirmed. Cancel leaves them as guesses.
8. **The Files tab also lists a file that brought no new conversations**, using its name as
   recorded at upload. Otherwise a later export that only added messages wouldn't appear at all.
9. **Some styling is still set by scripts.** All other styling moved to
   [timeline.css](../../frontend/timeline.css). What remains are values computed from data: a
   calendar bar's position, width and colour, and progress-bar widths.
10. **The deployed processing function's storage permission was widened** from `S3ReadPolicy` to
    `S3CrudPolicy` in [template.yaml](../../infra/template.yaml), so it can write the added messages.

## Faults found while building, and fixed

- **The export lost or duplicated messages in two cases.**
  - *Before this work, from reading the code:* uploading an older export after a newer one hid the
    newer messages, because the export rebuilt each conversation from the newest file only.
  - *Introduced by the first version of the new rebuild, caught by a test:* a later file that also
    brought new conversations replaced older conversations' original copies, so their added messages
    appeared twice. The rebuild now keys conversations by file and id.
- **The scan would never have read messages a later file added.** The plan had left it on the old
  file-reading path (C23). The export and the scan now share one rebuild.
- **The test routers gave the API and the development-only routes separate upload stores.** The real
  server shares one; nothing noticed until processing needed facts recorded by the upload route.
- **After a sign-in ran out, the Sign-in page still said "Signed in as…"** and hid the Sign in button.
- **After a scan failed, "Back to timeline" didn't appear**, although the files had been uploaded.
- **Different browser tests failed on different full runs.** The cause was the shared test helper,
  not the page: it returned when the timeline became visible, which now happens when the loading
  modal opens, before the data arrives. It now waits for the modal to close.

## Changes to existing tests beyond the approved list (§10b)

All are consequences of the approved plan. None weakens what a test checks.

- **Backend setup only:** two test doubles of the upload store pass the two new methods through, and
  four test routers share one upload store, as the real server does.
- **Publishing script tests** ([test-deploy-scripts.sh](../../scripts/test-deploy-scripts.sh)):
  - they expect the stylesheet among the published files, and five upload groups instead of four;
  - their example of a file with no known type changes from `style.css` to `picture.webp`, since
    `.css` now has a type.
- **Page unit tests:**
  - the restored notice's wording leaves the message-wording test;
  - the identifier scan in the catalog test also covers the `describe.` prefix, so it checks more
    identifiers, not fewer.
- **Browser tests:**
  - "pressing Load with no file chosen" signs in first;
  - "an unreachable backend is reported with a hint" blocks the backend after signing in, so it
    still tests the upload failing;
  - Sign out is clicked on the account line;
  - the missing-settings test checks that nothing past Sign-in can be reached;
  - the activity test expects the button label "Upload" and captures only the signed upload's PUT.

## Not done, or not working yet

- **Data already in the deployed AWS database can't be read after deploying this.** Rows stored
  before this work have no metadata. The server refuses them with a message saying why, rather than
  inventing file names and dates. The existing data must be cleared (or migrated) before or after
  deploying. **Needs a decision.**
- **Not deployed or checked on AWS.** That includes the processing function writing added messages
  with its widened permission, and Back on real Cognito's pages (plan C4).
- **Reading conversations from DynamoDB doesn't page through results.** This predates the work, but
  bigger rows make it more pressing. A DynamoDB query returns at most 1 MB per call, and
  `list_for_user` makes one call. With the metadata now on every row, a user with enough
  conversations (roughly over a thousand) would get a silently shortened list.
- **One small duplication.** "Get a token for the signed-in user" is a one-line call written out in
  three page modules, because the structure rules give them no shared module below them that may
  read the page.
