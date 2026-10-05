# Tests for the page lines no test runs

## Context

After every browser test records coverage
([plan](2026-10-02-browser-coverage-every-test.md)), these page lines still ran in no test, unit or
browser (measured 2026-10-05). The user asked (2026-10-05) for tests for all that can be tested,
unit tests acceptable, and a reason for any that can't.

| File | Lines | Case |
|---|---|---|
| [infra/api-client.js](../../../frontend/infra/api-client.js) | 59–63 | session id without `crypto.randomUUID` |
| | 122, 129–134 | the processed timeline's download answers an error, or never answers |
| | 153 | an activity batch's answer, with the page still open |
| | 247–248 | an error reply whose body can't be read |
| | 330–331, 338–339 | a flag save while signed out; for a message with no handle |
| [infra/cognito-login.js](../../../frontend/infra/cognito-login.js) | 18–19 | the sign-in library didn't load |
| | 70 | signing out |
| [ui/login-panel.js](../../../frontend/ui/login-panel.js) | 81–85 | sign-in fails to start |
| | 88–91 | signing out |
| [ui/annotated-export.js](../../../frontend/ui/annotated-export.js) | 17–19 | the download with nothing loaded |
| [ui/load-flow.js](../../../frontend/ui/load-flow.js) | 248–250 | receiving the timeline with no size given |
| [ui/widgets/status-indicators.js](../../../frontend/ui/widgets/status-indicators.js) | 113 | the time-remaining estimate |

## §1 Tests

**Unit** (`frontend/tests/`, Node's test runner; each file runs in its own process, so globals it
sets stay in it):
- `api-client-unit.test.js`: sets the two things the module reads as it loads (the address's query
  string, `localStorage`) and a stand-in `fetch`, then: `describeFailure` with a body that can't be
  read; `patchFlagsToBackend` signed out and with no handle; the export download answering 500 and
  failing with no answer (records the `request` event with the error's kind, throws); an activity
  batch's answer.
- `api-client-session-fallback.test.js`: removes `crypto.randomUUID` before loading the module; the
  session id is a version-4 UUID.
- `cognito-login.test.js`: `createCognitoLogin` without the sign-in library names the missing file.
- `annotated-export.test.js`: nothing loaded → the save line shows `export.no_data`. Not reachable
  from the page (the button is in the main view, shown only after a load, and nothing clears the
  loaded data); the function is tested directly.
- `rate-estimator.test.js`: `makeRateEstimator` with a stand-in clock gives a time remaining once a
  second has passed and something has moved.

**Browser** (`e2e/`):
- `activity.spec.js`: a batch is sent at a quiet moment while the page stays open, and its events
  reach the backend's log before the page closes (the case line 153 stands for).
- `cognito-login.spec.js`: sign out after signing in (both files' sign-out lines); sign-in that
  fails to start, by making the sign-in library's redirect fail inside the page, shows
  `signIn.start_failed`.
- `upload-flow.spec.js` or a new spec: the timeline download answered without a `Content-Length`
  (Playwright's `route.fulfill`), progress shows what arrived. **To check first:** whether
  `route.fulfill` can leave out `Content-Length`; if not, say so and use a unit test instead.

No program code changes, except where a test shows a fault; any such fault is reported first.

## Self-critique log

### C1 [RESOLVED]: stand-ins for browser globals could hide a real failure
Only what the module reads at load (query string, `localStorage`) and `fetch` are replaced; the
browser suite still runs the real page. **Resolution:** §1 lists exactly what each unit test
replaces.
