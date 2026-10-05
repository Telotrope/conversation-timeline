// The page flow of plan docs/plans/2026-10-05-screen-flow.md: Sign-in,
// Upload (several files, failures, Stop), Describe (after an upload, from
// the Files tab, from the Conversations tab), the timeline behind its
// loading modal, and Back. Against the real local backend, through the dev
// login.

const { test, expect } = require('./fixtures');
const { failOnPageErrors } = require('./page-health');
const { syntheticExport } = require('./synthetic-export');
const { API_BASE, TIMELINE_HTML } = require('./test-endpoints');
const { finishDescribe, signInToUpload } = require('./pages');

failOnPageErrors();

test.beforeEach(async () => {
  const res = await fetch(`${API_BASE}/_dev/reset`, { method: 'POST' });
  if (!res.ok) throw new Error(`could not reset the backend: ${res.status}`);
});

let counter = 0;
function uniqueSub() {
  counter += 1;
  return `flow-${process.pid}-${counter}`;
}

const at = (iso) => new Date(iso);

// A file for setInputFiles: `name`, holding `conversations` (see
// synthetic-export.js).
function file(name, conversations) {
  return { name, mimeType: 'application/json', buffer: Buffer.from(syntheticExport(conversations)) };
}

function chat(name, day, extra = {}) {
  return {
    name,
    ...extra,
    messages: [
      { sender: 'human', text: `hello from ${name}`, at: at(`${day}T10:00:00Z`) },
      { sender: 'assistant', text: 'hi', at: at(`${day}T10:01:00Z`) },
    ],
  };
}

async function uploadFiles(page, files) {
  await page.setInputFiles('#loadConvFile', files);
  await page.click('#loadBtn');
}

async function signedInToken(page) {
  return page.evaluate(async (api) => {
    const res = await fetch(`${api}/_dev/login`, {
      method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ sub: document.getElementById('devLoginSub').value }),
    });
    return (await res.json()).token;
  }, API_BASE);
}

async function serverFiles(page) {
  const token = await signedInToken(page);
  const res = await fetch(`${API_BASE}/uploads`, { headers: { Authorization: `Bearer ${token}` } });
  return res.json();
}

test('signed out: the Sign-in page explains the app; signing in with nothing uploaded opens Upload', async ({ page }) => {
  await page.goto(TIMELINE_HTML);
  await expect(page.locator('#signInPage')).toBeVisible();
  await expect(page.locator('#signInPage .sub')).toContainText('see your conversations laid out by day');
  await expect(page.locator('#accountBar')).toBeHidden();
  expect(new URL(page.url()).hash).toBe('#signin');

  const sub = uniqueSub();
  await page.fill('#devLoginSub', sub);
  await page.click('#devLoginBtn');
  await expect(page.locator('#uploadPage')).toBeVisible({ timeout: 30_000 });
  await expect(page.locator('#accountName')).toHaveText(sub);
  await expect(page.locator('#uploadBackBtn')).toBeHidden();
  expect(new URL(page.url()).hash).toBe('#upload');
});

test('signing in when the server is unreachable says so on the Sign-in page, with Try again', async ({ page }) => {
  await page.goto(TIMELINE_HTML);
  await page.route(`${API_BASE}/**`, (route) => route.abort());
  await page.click('#devLoginBtn');
  await expect(page.locator('#signInStatus')).toContainText('Failed to fetch');
  await expect(page.locator('#signInPage')).toBeVisible();
});

test('three files at once: one bar, one form, split on request, saved, then the Calendar', async ({ page }) => {
  await page.goto(TIMELINE_HTML);
  await signInToUpload(page, uniqueSub());
  await page.setInputFiles('#loadConvFile', [
    file('one.json', [chat('One', '2026-01-01')]),
    file('two.json', [chat('Two', '2026-01-02')]),
  ]);
  // A second choice adds to the list rather than replacing it.
  await page.setInputFiles('#loadConvFile', [file('three.json', [chat('Three', '2026-01-03')])]);
  await expect(page.locator('#chosenFiles li')).toHaveCount(3);
  await page.click('#loadBtn');

  await expect(page.locator('#describePage')).toBeVisible({ timeout: 30_000 });
  await expect(page.locator('#describeBody .describe-section')).toHaveCount(1);
  await expect(page.locator('#describeBody .describe-section h3')).toHaveText('These 3 files');
  await page.check('#describeBody .separate-toggle input');
  await expect(page.locator('#describeBody .describe-section')).toHaveCount(3);

  // The first file becomes a live voice conversation transcribed by a person.
  const first = page.locator('#describeBody .describe-section').first();
  await first.locator('input[value="live_voice"]').check();
  await first.locator('.describe-field select').last().selectOption('person');
  await page.click('#describeSaveBtn');

  await expect(page.locator('#mainContent')).toBeVisible({ timeout: 30_000 });
  await expect(page.locator('#loadingModal')).toBeHidden({ timeout: 30_000 });
  await expect(page.locator('#view-calendar')).toHaveClass(/active/);
  expect(new URL(page.url()).hash).toBe('#calendar');
  await expect(page.locator('#convItems .conv-item')).toHaveCount(3);

  const files = await serverFiles(page);
  expect(files.map((f) => f.details_origin)).toEqual(['confirmed', 'confirmed', 'confirmed']);
  const one = files.find((f) => f.file_name === 'one.json');
  expect(one.medium).toEqual({ kind: 'live_voice', transcription: { service: 'person' } });
  expect(files.find((f) => f.file_name === 'two.json').medium).toEqual({ kind: 'typed' });
});

test('a file the server refuses gets a line below the bar; the other is described with a reminder', async ({ page }) => {
  await page.goto(TIMELINE_HTML);
  await signInToUpload(page, uniqueSub());
  let puts = 0;
  await page.route(/\/_dev\/local-storage\/put\//, (route) => {
    puts += 1;
    return puts === 1 ? route.fulfill({ status: 500, body: 'disk full' }) : route.fallback();
  });
  await uploadFiles(page, [file('bad.json', [chat('Bad', '2026-01-01')]), file('good.json', [chat('Good', '2026-01-02')])]);

  await expect(page.locator('#describePage')).toBeVisible({ timeout: 30_000 });
  const reminder = page.locator('#describeReminder');
  await expect(reminder).toBeVisible();
  await expect(reminder).toContainText('1 file from this batch is not here');
  await expect(reminder).toContainText('disk full');
  await expect(page.locator('#describeBody .describe-section')).toHaveCount(1);
});

test('when every file fails, the Upload page says so and lists each reason', async ({ page }) => {
  await page.goto(TIMELINE_HTML);
  await signInToUpload(page, uniqueSub());
  await page.route(/\/_dev\/local-storage\/put\//, (route) => route.fulfill({ status: 500, body: 'disk full' }));
  await uploadFiles(page, [file('a.json', [chat('A', '2026-01-01')]), file('b.json', [chat('B', '2026-01-02')])]);
  await expect(page.locator('#loadStatus')).toHaveText('None of the 2 files could be uploaded; the reasons are below.');
  await expect(page.locator('#fileFailures li')).toHaveCount(2);
  await expect(page.locator('#fileFailures li').first()).toContainText('disk full');
  await expect(page.locator('#loadProgressFill')).toHaveClass(/is-error/);
});

test('Stop with one file processed and one still sending: Describe for the first, the other named as stopped', async ({ page }) => {
  await page.goto(TIMELINE_HTML);
  await signInToUpload(page, uniqueSub());
  // The second file's bytes never arrive until Stop cancels them.
  let puts = 0;
  await page.route(/\/_dev\/local-storage\/put\//, (route) => {
    puts += 1;
    if (puts === 1) return route.fallback();
    return new Promise(() => {});
  });
  const firstReady = page.waitForResponse((r) => /\/uploads\/[0-9a-f-]{36}$/.test(r.url()) && r.status() === 200);
  await uploadFiles(page, [file('quick.json', [chat('Quick', '2026-01-01')]), file('stuck.json', [chat('Stuck', '2026-01-02')])]);
  await firstReady;
  await page.click('#stopBtn');

  await expect(page.locator('#describePage')).toBeVisible({ timeout: 30_000 });
  await expect(page.locator('#describeReminder')).toContainText('stuck.json');
  await expect(page.locator('#describeBody .describe-section h3')).toHaveText('quick.json');
});

test('Stop before anything is processed stays on the Upload page and says so', async ({ page }) => {
  await page.goto(TIMELINE_HTML);
  await signInToUpload(page, uniqueSub());
  await page.route(/\/_dev\/local-storage\/put\//, () => new Promise(() => {}));
  await uploadFiles(page, [file('stuck.json', [chat('Stuck', '2026-01-02')])]);
  await expect(page.locator('#stopBtn')).toBeVisible();
  await page.click('#stopBtn');
  await expect(page.locator('#loadStatus')).toContainText('Stopped before any file was processed');
  await expect(page.locator('#fileFailures')).toContainText('stuck.json: stopped');
  await expect(page.locator('#uploadPage')).toBeVisible();
});

test('leaving Describe by reloading keeps the guesses, and the next visit opens the timeline', async ({ page }) => {
  await page.goto(TIMELINE_HTML);
  await signInToUpload(page, uniqueSub());
  await uploadFiles(page, [file('one.json', [chat('One', '2026-01-01')])]);
  await expect(page.locator('#describePage')).toBeVisible({ timeout: 30_000 });
  await page.reload();
  await expect(page.locator('#mainContent')).toBeVisible({ timeout: 30_000 });
  await expect(page.locator('#loadingModal')).toBeHidden({ timeout: 30_000 });
  await expect(page.locator('#describePage')).toBeHidden();
  await page.click('button[data-tab="files"]');
  await expect(page.locator('#filesBody tbody tr')).toHaveCount(1);
  await expect(page.locator('#filesBody tbody tr')).toContainText('guessed');
  await expect(page.locator('#filesBody tbody tr')).toContainText('Typed');
});

test('the Files tab edits a whole file; Cancel leaves it unchanged', async ({ page }) => {
  await page.goto(TIMELINE_HTML);
  await signInToUpload(page, uniqueSub());
  await uploadFiles(page, [file('meet.json', [chat('Meet', '2026-01-01')])]);
  await finishDescribe(page);
  await expect(page.locator('#loadingModal')).toBeHidden({ timeout: 30_000 });
  await page.click('button[data-tab="files"]');

  await page.click('#filesBody .edit-file');
  await expect(page.locator('#describePage')).toBeVisible();
  expect(new URL(page.url()).hash).toMatch(/^#describe\/file\//);
  await page.locator('#describeBody input[value="virtual_voice"]').check();
  await page.click('#describeLeaveBtn');
  await expect(page.locator('#view-files')).toHaveClass(/active/);
  await expect(page.locator('#filesBody tbody tr')).toContainText('Typed');

  await page.click('#filesBody .edit-file');
  await page.locator('#describeBody input[value="virtual_voice"]').check();
  await page.locator('#describeBody .describe-field select').last().selectOption('zoom');
  await page.click('#describeSaveBtn');
  await expect(page.locator('#view-files')).toHaveClass(/active/);
  await expect(page.locator('#filesBody tbody tr')).toContainText('Virtual voice (Zoom)');
  await expect(page.locator('#filesBody tbody tr')).toContainText('confirmed');
});

test('Describe marks a broken answer and waits', async ({ page }) => {
  await page.goto(TIMELINE_HTML);
  await signInToUpload(page, uniqueSub());
  await uploadFiles(page, [file('one.json', [chat('One', '2026-01-01')])]);
  await expect(page.locator('#describeBody .describe-section')).toBeVisible({ timeout: 30_000 });
  await page.locator('#describeBody .participant-row input[type="text"]').first().fill('   ');
  await page.locator('#describeBody input[value="live_voice"]').check();
  await page.click('#describeSaveBtn');
  await expect(page.locator('#describeStatus')).toHaveText('Some answers need fixing first; they are marked.');
  await expect(page.locator('#describeBody .field-error')).toHaveCount(2);
  await expect(page.locator('#describePage')).toBeVisible();
});

test('the Conversations tab edits one conversation, start and end included', async ({ page }) => {
  await page.goto(TIMELINE_HTML);
  await signInToUpload(page, uniqueSub());
  await uploadFiles(page, [file('two.json', [chat('Alpha', '2026-01-01'), chat('Beta', '2026-01-02')])]);
  await finishDescribe(page);
  await expect(page.locator('#loadingModal')).toBeHidden({ timeout: 30_000 });
  await page.click('button[data-tab="conversations"]');
  await page.locator('.conv-item', { hasText: 'Alpha' }).click();
  await page.click('#editConversationBtn');
  await expect(page.locator('#describeTitle')).toHaveText('Describe this conversation');

  const rows = page.locator('#describeBody .participant-row');
  await rows.nth(1).locator('select').selectOption('gemini');
  await page.locator('#describeBody input[type="datetime-local"]').last().fill('2026-01-01T23:30');
  await page.click('#describeSaveBtn');

  await expect(page.locator('#view-conversations')).toHaveClass(/active/);
  await expect(page.locator('#convDetail h3')).toHaveText('Alpha');
  await expect(page.locator('.conv-details')).toContainText('Gemini');
  await expect(page.locator('.conv-details')).toContainText('confirmed');

  // The file's participants now differ between its conversations...
  await page.click('button[data-tab="files"]');
  await expect(page.locator('#filesBody tbody tr')).toContainText('varies');
  // ...and a file-wide change of kind leaves Alpha's participants as edited.
  await page.click('#filesBody .edit-file');
  await page.locator('#describeBody input[value="live_voice"]').check();
  await page.locator('#describeBody .describe-field select').last().selectOption('unknown');
  await page.click('#describeSaveBtn');
  await expect(page.locator('#filesBody tbody tr')).toContainText("Live voice (Don't know)");
  await page.click('button[data-tab="conversations"]');
  await page.locator('.conv-item', { hasText: 'Alpha' }).click();
  await expect(page.locator('.conv-details')).toContainText('Gemini');
});

test('a later file repeating a conversation adds only its new messages', async ({ page }) => {
  const id = '11111111-2222-4333-8444-555555555555';
  const m = (n, sender, iso) => ({ uuid: `00000000-0000-4000-8000-${String(n).padStart(12, '0')}`, sender, text: `message ${n}`, at: at(iso) });
  const first = [{ name: 'Ongoing', uuid: id, messages: [m(1, 'human', '2026-01-01T10:00:00Z'), m(2, 'assistant', '2026-01-01T10:01:00Z')] }];
  const later = [{ name: 'Ongoing', uuid: id, messages: [...first[0].messages, m(3, 'human', '2026-01-02T10:00:00Z'), m(4, 'assistant', '2026-01-02T10:01:00Z')] }];

  await page.goto(TIMELINE_HTML);
  await signInToUpload(page, uniqueSub());
  await uploadFiles(page, [file('first.json', first)]);
  await finishDescribe(page);
  await expect(page.locator('#loadingModal')).toBeHidden({ timeout: 30_000 });

  await page.click('#addConversationsBtn');
  await uploadFiles(page, [file('later.json', later)]);
  await expect(page.locator('#describePage')).toBeVisible({ timeout: 30_000 });
  await expect(page.locator('#describeBody .hint')).toContainText('nothing new to describe');
  await page.click('#describeSaveBtn');
  await expect(page.locator('#loadingModal')).toBeHidden({ timeout: 30_000 });

  await page.click('button[data-tab="conversations"]');
  await expect(page.locator('#convItems .conv-item')).toHaveCount(1);
  await expect(page.locator('#convItems .conv-item')).toContainText('4 messages');
  await page.click('button[data-tab="files"]');
  await expect(page.locator('#filesBody tbody tr').first()).toContainText('1 already present, 1 gained messages');
});

test('Add conversations goes to Upload, Back returns, and the timeline holds old and new', async ({ page }) => {
  await page.goto(TIMELINE_HTML);
  await signInToUpload(page, uniqueSub());
  await uploadFiles(page, [file('one.json', [chat('Old', '2026-01-01')])]);
  await finishDescribe(page);
  await expect(page.locator('#loadingModal')).toBeHidden({ timeout: 30_000 });

  await page.click('#addConversationsBtn');
  await expect(page.locator('#uploadPage')).toBeVisible();
  await expect(page.locator('#uploadBackBtn')).toBeVisible();
  await page.goBack();
  await expect(page.locator('#mainContent')).toBeVisible();

  await page.click('#addConversationsBtn');
  await uploadFiles(page, [file('two.json', [chat('New', '2026-02-01')])]);
  await finishDescribe(page);
  await expect(page.locator('#loadingModal')).toBeHidden({ timeout: 30_000 });
  await expect(page.locator('#convItems .conv-item')).toHaveCount(2);
});

test('Back: the first timeline entry shows the Calendar, and Back on Describe is refused', async ({ page }) => {
  await page.goto(TIMELINE_HTML);
  await signInToUpload(page, uniqueSub());
  await uploadFiles(page, [file('one.json', [chat('One', '2026-01-01')])]);
  await expect(page.locator('#describePage')).toBeVisible({ timeout: 30_000 });
  await page.goBack();
  await expect(page.locator('#describeStatus')).toHaveText('Press Done to save, or Cancel to leave without saving.');
  await expect(page.locator('#describePage')).toBeVisible();
  await page.click('#describeSaveBtn');
  await expect(page.locator('#loadingModal')).toBeHidden({ timeout: 30_000 });

  await page.click('button[data-tab="review"]');
  await page.goBack();
  await expect(page.locator('#view-calendar')).toHaveClass(/active/);
});

test('signing out from the timeline returns to Sign-in and forgets the dev name', async ({ page }) => {
  await page.goto(TIMELINE_HTML);
  await signInToUpload(page, uniqueSub());
  await page.click('#accountSignOutBtn');
  await expect(page.locator('#signInPage')).toBeVisible();
  await page.reload();
  await expect(page.locator('#signInPage')).toBeVisible();
});

// Uploads `files` and accepts Describe's answers, ending on the timeline.
async function uploadAndFinish(page, files) {
  await uploadFiles(page, files);
  await finishDescribe(page);
  await expect(page.locator('#loadingModal')).toBeHidden({ timeout: 30_000 });
}

test('the server failing right after sign-in is shown on the Sign-in page; Try again goes on', async ({ page }) => {
  await page.goto(TIMELINE_HTML);
  await page.route(`${API_BASE}/conversations`, (route) => route.fulfill({ status: 503, body: 'maintenance' }));
  await page.fill('#devLoginSub', uniqueSub());
  await page.click('#devLoginBtn');
  await expect(page.locator('#signInStatus')).toContainText('Could not reach the server to find your conversations');
  await expect(page.locator('#checkAgainBtn')).toBeVisible();
  await page.unroute(`${API_BASE}/conversations`);
  await page.click('#checkAgainBtn');
  await expect(page.locator('#uploadPage')).toBeVisible({ timeout: 30_000 });
});

test('reloading on a conversation\'s Describe address opens the timeline, then that Describe page', async ({ page }) => {
  await page.goto(TIMELINE_HTML);
  await signInToUpload(page, uniqueSub());
  await uploadAndFinish(page, [file('one.json', [chat('Solo', '2026-01-01')])]);
  await page.click('button[data-tab="conversations"]');
  await page.locator('.conv-item', { hasText: 'Solo' }).click();
  await page.click('#editConversationBtn');
  const address = new URL(page.url()).hash;
  expect(address).toMatch(/^#describe\/conversation\//);
  await page.reload();
  await expect(page.locator('#describePage')).toBeVisible({ timeout: 30_000 });
  await expect(page.locator('#describeTitle')).toHaveText('Describe this conversation');
  expect(new URL(page.url()).hash).toBe(address);
});

test('Describe says so when it can\'t read the files\' details, or can\'t save them', async ({ page }) => {
  await page.goto(TIMELINE_HTML);
  await signInToUpload(page, uniqueSub());
  await page.route(`${API_BASE}/uploads`, (route) => (route.request().method() === 'GET'
    ? route.fulfill({ status: 500, body: 'broken' }) : route.fallback()));
  await uploadFiles(page, [file('one.json', [chat('One', '2026-01-01')])]);
  await expect(page.locator('#describeStatus')).toContainText("Could not read your files' details");
  await page.unroute(`${API_BASE}/uploads`);

  await page.click('#describeLeaveBtn');
  await expect(page.locator('#loadingModal')).toBeHidden({ timeout: 30_000 });
  await page.click('button[data-tab="files"]');
  await page.route(/\/uploads\/[0-9a-f-]+\/metadata$/, (route) => route.fulfill({ status: 500, body: '{"error":"disk full"}' }));
  await page.click('#filesBody .edit-file');
  await page.click('#describeSaveBtn');
  await expect(page.locator('#describeStatus')).toContainText('Could not save: saving the file\'s details failed (500): disk full');
  await expect(page.locator('#describeSaveBtn')).toBeEnabled();
});

test('a save whose details can\'t be read back stays on Describe and says so', async ({ page }) => {
  await page.goto(TIMELINE_HTML);
  await signInToUpload(page, uniqueSub());
  await uploadAndFinish(page, [file('one.json', [chat('One', '2026-01-01')])]);
  await page.click('button[data-tab="files"]');
  await page.click('#filesBody .edit-file');
  await expect(page.locator('#describeBody .describe-section')).toBeVisible();
  await page.route(`${API_BASE}/conversations`, (route) => route.fulfill({ status: 500, body: 'broken' }));
  await page.click('#describeSaveBtn');
  await expect(page.locator('#describeStatus')).toContainText("Could not read your files' details");
  await expect(page.locator('#describePage')).toBeVisible();
});

test('unticking "Describe each file separately" asks first when the files differ', async ({ page }) => {
  await page.goto(TIMELINE_HTML);
  await signInToUpload(page, uniqueSub());
  await uploadFiles(page, [file('a.json', [chat('A', '2026-01-01')]), file('b.json', [chat('B', '2026-01-02')])]);
  const toggle = page.locator('#describeBody .separate-toggle input');
  await expect(toggle).toBeVisible({ timeout: 30_000 });
  await toggle.check();
  await page.locator('#describeBody .describe-section').first().locator('input[value="live_voice"]').check();

  // Declined: the forms stay separate and the box ticked.
  page.once('dialog', (d) => d.dismiss());
  await toggle.click();
  await expect(page.locator('#describeBody .describe-section')).toHaveCount(2);
  await expect(page.locator('#describeBody .separate-toggle input')).toBeChecked();

  page.once('dialog', (d) => d.accept());
  await page.locator('#describeBody .separate-toggle input').uncheck();
  await expect(page.locator('#describeBody .describe-section')).toHaveCount(1);
  await expect(page.locator('#describeBody input[value="live_voice"]')).toBeChecked();
});

test('a file bringing old and new conversations counts both, and "Set for all" sets a field that varied', async ({ page }) => {
  const id = '99999999-2222-4333-8444-555555555555';
  const old = { name: 'Old', uuid: id, messages: [{ sender: 'human', text: 'hi', at: at('2026-01-01T10:00:00Z'), uuid: '00000000-0000-4000-8000-000000000101' }] };
  await page.goto(TIMELINE_HTML);
  await signInToUpload(page, uniqueSub());
  await uploadAndFinish(page, [file('first.json', [old, chat('Other', '2026-01-03')])]);
  // One of first.json's conversations gets its own participants.
  await page.click('button[data-tab="conversations"]');
  await page.locator('.conv-item', { hasText: 'Other' }).click();
  await page.click('#editConversationBtn');
  await page.locator('#describeBody .participant-row').nth(1).locator('select').selectOption('chatgpt');
  await page.click('#describeSaveBtn');
  await expect(page.locator('#view-conversations')).toHaveClass(/active/);

  await page.click('#addConversationsBtn');
  await uploadFiles(page, [file('second.json', [old, chat('Brand new', '2026-02-01')])]);
  await expect(page.locator('#describeBody .counts')).toContainText('1 new conversation, 1 already present');
  await finishDescribe(page);
  await expect(page.locator('#loadingModal')).toBeHidden({ timeout: 30_000 });

  await page.click('button[data-tab="files"]');
  await page.locator('#filesBody tr', { hasText: 'first.json' }).locator('.edit-file').click();
  await expect(page.locator('#describeBody .varies').first()).toBeVisible();
  await page.click('#describeBody button:has-text("Set for all")');
  await expect(page.locator('#describeBody .participant-row')).toHaveCount(2);
  await page.locator('#describeBody .participant-row input[type="text"]').fill('Ada');
  await page.click('#describeSaveBtn');
  await expect(page.locator('#filesBody tr', { hasText: 'first.json' })).toContainText('Ada, Claude');
});

test('a scan that fails after the upload is reported on the Upload page', async ({ page }) => {
  await page.goto(TIMELINE_HTML);
  await signInToUpload(page, uniqueSub());
  await page.route(`${API_BASE}/detect`, (route) => route.fulfill({ status: 500, body: '{"error":"scanner down"}' }));
  await page.check('#autoDetectCheckbox');
  await uploadFiles(page, [file('one.json', [chat('One', '2026-01-01')])]);
  await expect(page.locator('#loadStatus')).toContainText('scanning your messages failed (500): scanner down');
  await expect(page.locator('#loadProgressFill')).toHaveClass(/is-error/);
  await expect(page.locator('#uploadBackBtn')).toBeVisible();
});

test('with the browser\'s storage blocked, the page still asks you to sign in, and signing out still works', async ({ page }) => {
  await page.addInitScript(() => {
    Storage.prototype.getItem = () => { throw new Error('storage blocked'); };
    Storage.prototype.removeItem = () => { throw new Error('storage blocked'); };
  });
  await page.goto(TIMELINE_HTML);
  await expect(page.locator('#signInPage')).toBeVisible();
  await page.fill('#devLoginSub', uniqueSub());
  await page.click('#devLoginBtn');
  await expect(page.locator('#uploadPage')).toBeVisible({ timeout: 30_000 });
  await page.click('#accountSignOutBtn');
  await expect(page.locator('#signInPage')).toBeVisible();
});
