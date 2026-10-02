// Where the browser tests find the page and the backend. One place, so the
// spec files can't drift apart on a port again.
//
// The backend runs on 3123, a port only the test run uses: never 3000,
// where the dev launcher (scripts/dev-up.sh) runs your own backend. On
// 2026-10-01 the tests silently talked to an old dev backend on 3000 and
// failed for reasons that had nothing to do with the code under test; see
// docs/plans/completed/2026-10-01-browser-tests-own-server.md.

const BACKEND_PORT = 3123;
const API_BASE = `http://127.0.0.1:${BACKEND_PORT}`;

// The page itself, with no query string -- for tests that build their own.
const PAGE_URL = 'http://127.0.0.1:8123/timeline.html';

// The page pointed at the test backend through its `api_base` parameter.
const TIMELINE_HTML = `${PAGE_URL}?api_base=${encodeURIComponent(API_BASE)}`;

module.exports = { BACKEND_PORT, API_BASE, PAGE_URL, TIMELINE_HTML };
