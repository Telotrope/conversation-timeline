// The page's activity log (docs/plans/2026-10-02-activity-instrumentation.md
// §4, §8 and §10), in a real browser against the real local backend:
//
// - what recording costs each click, measured on the review table, with the
//   plan's limit of 4 ms per click, a quarter of one screen redraw (the
//   user's limit; 1 ms at first, which no one could notice either);
// - one session's records, as the local backend logs them to its standard
//   output: in order, with one session id, and with no message text.
//
// The backend's standard output is kept in a file by backend-server.js.

const fs = require('fs');
const { test, expect } = require('@playwright/test');
const { failOnPageErrors } = require('./page-health');
const { collectCoverage } = require('./coverage');
const { syntheticExport } = require('./synthetic-export');
const { API_BASE, TIMELINE_HTML } = require('./test-endpoints');

failOnPageErrors();
collectCoverage();

const S3_PUT_ROUTE = 's3 PUT raw/…';

// Words found nowhere else, so finding one in the log can only mean message
// text or a conversation's name leaked into it.
const CONVERSATIONS = Array.from({ length: 7 }, (_, c) => ({
  name: `Quokka dossier ${c} vermilion`,
  messages: [0, 1, 2].flatMap((m) => [
    {
      sender: 'human',
      text: `Zephyrine marmalade ${c}-${m} WHISPERS NOTHING IS FINE quillwort`,
      at: new Date(Date.UTC(2024, 0, 1 + c, 9, m * 5)),
    },
    {
      sender: 'assistant',
      text: `Obsidian heronry reply ${c}-${m} lanternfish`,
      at: new Date(Date.UTC(2024, 0, 1 + c, 9, m * 5 + 1)),
    },
  ]),
}));
const SECRETS = CONVERSATIONS.flatMap((c) => [c.name, ...c.messages.map((m) => m.text)]);

async function resetBackend() {
  const res = await fetch(`${API_BASE}/_dev/reset`, { method: 'POST' });
  if (!res.ok) throw new Error(`could not reset the backend: ${res.status}`);
}

let loginCounter = 0;
function uniqueSub() {
  loginCounter += 1;
  return `activity-${process.pid}-${loginCounter}`;
}

async function loadExport(page, { sub, scan }) {
  await page.goto(TIMELINE_HTML);
  await page.fill('#devLoginSub', sub);
  if (scan) await page.check('#autoDetectCheckbox');
  await page.setInputFiles('#loadConvFile', {
    name: 'conversations.json',
    mimeType: 'application/json',
    buffer: Buffer.from(syntheticExport(CONVERSATIONS)),
  });
  await page.click('#loadBtn');
  await expect(page.locator('#mainContent')).toBeVisible({ timeout: 30_000 });
}

// The backend's log lines, parsed; lines that aren't JSON are skipped.
function backendLines() {
  const file = process.env.E2E_BACKEND_STDOUT;
  if (!file) throw new Error('E2E_BACKEND_STDOUT is not set: backend-server.js should have set it');
  const text = fs.readFileSync(file, 'utf8');
  return {
    text,
    parsed: text.split('\n').flatMap((line) => {
      // Plain-text lines (the server's own messages) are not records.
      try { return [JSON.parse(line)]; } catch (e) { return []; }
    }),
  };
}

test.beforeEach(async () => {
  await resetBackend();
});

test('recording a click on the review table takes under 4 ms', async ({ page }) => {
  await loadExport(page, { sub: uniqueSub(), scan: false });
  await page.click('button[data-tab="review"]');
  await expect(page.locator('#reviewTable .approve-btn').first()).toBeVisible();

  // Times the document's capture-phase listeners, which run between a
  // capture listener on the window (added here, first on the click's path)
  // and one on the review table (added here, after the document on the
  // path). The only capture listener on the document is the recorder's.
  // The table's listener then stops the click and cancels it, so the page's
  // own handlers (saving a flag) don't run and checkboxes don't toggle.
  //
  // Chrome rounds performance.now() to 0.1 ms on this page, so each click's
  // time is to the nearest 0.1 ms; the average per click is also measured,
  // more finely, over the whole run (all 1,000 clicks with the recorder,
  // less the same clicks stopped at the window, before it).
  const { times, meanMs } = await page.evaluate(() => {
    const table = document.getElementById('reviewTable');
    const targets = [...table.querySelectorAll(
      '.flag-checkbox input, .approve-btn, td.msg-text, td.conv-name, td.when'
    )];
    const times = [];
    let started = 0;
    const start = () => { started = performance.now(); };
    const stop = (e) => {
      times.push(performance.now() - started);
      e.stopPropagation();
      e.preventDefault();
    };
    window.addEventListener('click', start, true);
    table.addEventListener('click', stop, true);
    const clickAll = () => {
      const began = performance.now();
      for (let i = 0; i < 1000; i++) {
        targets[i % targets.length].dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }));
      }
      return performance.now() - began;
    };
    const withRecorder = clickAll();
    window.removeEventListener('click', start, true);
    const stopFirst = (e) => { e.stopPropagation(); e.preventDefault(); };
    window.addEventListener('click', stopFirst, true);
    const withoutRecorder = clickAll();
    window.removeEventListener('click', stopFirst, true);
    table.removeEventListener('click', stop, true);
    return { times, meanMs: (withRecorder - withoutRecorder) / 1000 };
  });

  expect(times.length).toBe(1000);
  const sorted = [...times].sort((a, b) => a - b);
  const median = sorted[500];
  const slowest = sorted[sorted.length - 1];
  const slow = times.map((t, i) => [i, t]).filter(([, t]) => t >= 0.3);
  console.log(`clicks at or over 0.3 ms (index, ms): ${JSON.stringify(slow)}`);
  console.log(
    `recording one click, over 1,000 clicks: median ${median.toFixed(1)} ms (to the nearest 0.1 ms), ` +
    `average ${(meanMs * 1000).toFixed(1)} µs, slowest ${slowest.toFixed(1)} ms`
  );
  expect(slowest, `slowest click took ${slowest} ms`).toBeLessThan(4);
});

test("one session's activity reaches the backend's log, in order, without message text", async ({ page }) => {
  const sub = uniqueSub();
  const sessionHeaders = { uploads: undefined, put: 'not seen' };
  page.on('request', (req) => {
    const url = req.url();
    if (req.method() === 'POST' && url === `${API_BASE}/uploads`) sessionHeaders.uploads = req.headers()['x-timeline-session'];
    if (req.method() === 'PUT') sessionHeaders.put = req.headers()['x-timeline-session'];
  });

  await loadExport(page, { sub, scan: true });

  await page.click('button[data-tab="review"]');
  const approve = page.locator('#reviewTable .approve-btn').first();
  await expect(approve).toBeVisible();
  const messageId = await approve.getAttribute('data-id');
  await approve.click();
  await expect(page.locator('#saveStatus')).toHaveText('Saved.', { timeout: 10_000 });
  await page.click('button[data-tab="calendar"]');

  // The page's own wording: every control's text and the status lines, as
  // shown now, plus the status messages the flow showed along the way. None
  // of it may reach the records (plan §4, C17); 4 characters or more, so a
  // one-letter flag icon can't match inside an id.
  const pageWording = (await page.evaluate(() => [
    ...document.querySelectorAll('button, label, option, #loadStatus, #saveStatus, #loadProgressLabel'),
  ].map((el) => el.textContent.trim())))
    .concat(['Saved.', 'Logging in…', 'Sending your file…', 'Processing on the server…',
      'Scanning your messages for flags…', 'Preparing the timeline…', 'Signing in…', 'Reading the file…'])
    .filter((w) => w.length >= 4);
  expect(pageWording).toContain('Load');
  expect(pageWording).toContain('Approve');

  // Leaving the page sends what is waiting (pagehide, with keepalive).
  await page.goto('about:blank');

  let events = [];
  await expect.poll(() => {
    const { parsed } = backendLines();
    events = parsed.filter((l) => l.kind === 'page_event' && l.user === sub);
    return events.some((l) => l.event.kind === 'click' && l.event.target.tab === 'calendar');
  }, { timeout: 15_000, message: `page_event lines for ${sub} in the backend's output` }).toBe(true);

  // One session id on every record, the one the page sent to the API, and
  // none sent to the signed upload link.
  const sessions = new Set(events.map((l) => l.session));
  expect([...sessions]).toEqual([sessionHeaders.uploads]);
  expect(sessionHeaders.uploads).toMatch(/^[0-9a-f-]{36}$/);
  expect(sessionHeaders.put).toBeUndefined();

  // In order: each expectation is found after the previous one.
  const e = events.map((l) => l.event);
  const steps = [
    ['click on the scan box', (x) => x.kind === 'click' && x.target.id === 'autoDetectCheckbox'],
    ['click on Load', (x) => x.kind === 'click' && x.target.id === 'loadBtn'],
    ['POST /uploads with the scan box ticked', (x) => x.kind === 'request' && x.method === 'POST' && x.route === '/uploads' && x.scan === true && x.status === 200],
    ['the S3 PUT', (x) => x.kind === 'request' && x.method === 'PUT' && x.route === S3_PUT_ROUTE && x.status === 200],
    ['POST /detect, first page', (x) => x.kind === 'request' && x.route === '/detect' && x.offset === 0 && x.limit === 5],
    ['POST /detect, second page', (x) => x.kind === 'request' && x.route === '/detect' && x.offset === 5 && x.limit === 5],
    ['click on the review tab', (x) => x.kind === 'click' && x.target.tab === 'review'],
    ['click on Approve, with its message id', (x) => x.kind === 'click' && x.target.message_id === messageId && x.target.column === 'approve'],
    ['the flag save', (x) => x.kind === 'request' && x.method === 'PATCH' && x.route === '/conversations/{id}/messages/{id}/flags' && x.status === 200],
    ['"Saved." shown, by its identifier', (x) => x.kind === 'shown' && x.where === 'saveStatus' && x.message === 'save.saved'],
    ['click on the calendar tab', (x) => x.kind === 'click' && x.target.tab === 'calendar'],
  ];
  let at = -1;
  for (const [what, matches] of steps) {
    const found = e.findIndex((x, i) => i > at && matches(x));
    expect(found, `${what}, after record ${at}, in:\n${e.map((x) => JSON.stringify(x)).join('\n')}`).toBeGreaterThan(at);
    at = found;
  }

  // Every record carries the page's version: 'local' with no deployment.
  expect(e.filter((x) => x.page_version !== 'local')).toEqual([]);

  // No record holds the page's wording: no `label` or `text` from the page,
  // and none of the wording collected above.
  for (const x of e) {
    expect(x.target && 'label' in x.target, JSON.stringify(x)).toBeFalsy();
    if (x.kind === 'shown') expect('text' in x, JSON.stringify(x)).toBe(false);
  }
  const recordLines = backendLines().text.split('\n').filter((l) => l.includes('"page_event"'));
  const wordingFound = pageWording.filter((w) => recordLines.some((l) => l.includes(w)));
  expect(wordingFound).toEqual([]);

  // No line of the backend's output holds a message's text or a
  // conversation's name.
  const { text } = backendLines();
  const leaked = SECRETS.filter((s) => text.includes(s));
  expect(leaked).toEqual([]);
  for (const word of ['Zephyrine', 'Quokka', 'Obsidian']) expect(text).not.toContain(word);
});
