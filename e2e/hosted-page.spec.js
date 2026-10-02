// The hosted page (docs/plans/2026-10-02-page-hosting.md §2): served at `/`
// as index.html, with a timeline-deploy tag naming its deployment, the way
// scripts/publish-page.sh publishes it. The tag here is added the same way
// (after the charset line); scripts/test-deploy-scripts.sh checks the script
// itself adds it. Real hosting is deployment checks H1-H10.
//
// The test server serves the repo root, so the page's relative addresses
// (frontend/main.js, vendor/...) resolve below `/` exactly as on CloudFront.
// Playwright can't intercept the address Cognito redirects back to, so the
// sign-in round trip uses a small server of its own that answers `/` with
// the tagged page.

const fs = require('fs');
const http = require('http');
const path = require('path');
const { test, expect } = require('@playwright/test');
const { failOnPageErrors } = require('./page-health');
const { startCognitoStandIn } = require('./cognito-standin');
const { API_BASE, PAGE_URL } = require('./test-endpoints');

const ROOT_URL = new URL('/', PAGE_URL).href;
const REPO = path.resolve(__dirname, '..');
const PAGE_SOURCE = fs.readFileSync(path.join(REPO, 'timeline.html'), 'utf8');
const TYPES = { '.js': 'text/javascript', '.json': 'application/json', '.html': 'text/html' };

function tagged(name) {
  const charset = '<meta charset="UTF-8">\n';
  return PAGE_SOURCE.replace(charset, `${charset}<meta name="timeline-deploy" content="${name}">\n`);
}

// Serves `/` as the published index.html for deployment `name`, and the
// repo's files below it. Resolves to { url, close }.
function startHostedServer(name) {
  const server = http.createServer((req, res) => {
    const { pathname } = new URL(req.url, 'http://x');
    if (pathname === '/') {
      res.writeHead(200, { 'content-type': 'text/html; charset=utf-8' });
      return res.end(tagged(name));
    }
    const file = path.resolve(REPO, `.${decodeURIComponent(pathname)}`);
    if (!file.startsWith(`${REPO}${path.sep}`) || !fs.existsSync(file) || !fs.statSync(file).isFile()) {
      res.writeHead(404);
      return res.end();
    }
    res.writeHead(200, { 'content-type': TYPES[path.extname(file)] || 'text/plain' });
    fs.createReadStream(file).pipe(res);
  });
  return new Promise((resolve) => server.listen(0, '127.0.0.1', () => resolve({
    url: `http://127.0.0.1:${server.address().port}/`,
    close: () => new Promise((done) => server.close(done)),
  })));
}

const CLIENT_ID = 'e2eclient123';
const EMAIL = 'hosted@example.com';

let standIn;

test.beforeAll(async () => {
  standIn = await startCognitoStandIn({ clientId: CLIENT_ID, apiBase: API_BASE, sub: 'cognito-hosted', email: EMAIL });
});
test.afterAll(async () => { await standIn.close(); });

test.beforeEach(async () => {
  const res = await fetch(`${API_BASE}/_dev/reset`, { method: 'POST' });
  if (!res.ok) throw new Error(`could not reset the backend: ${res.status}`);
});

failOnPageErrors();

const status = (page) => page.locator('#cognitoLoginStatus');

// Serves `/` as the published index.html with tag content `name`, answers
// every deployment's settings file, and returns the settings files asked for.
async function openHosted(page, name, query = '', root = ROOT_URL) {
  if (root === ROOT_URL) {
    await page.route((url) => url.href.split('?')[0] === ROOT_URL, (route) => route.fulfill({
      contentType: 'text/html; charset=utf-8',
      body: tagged(name),
    }));
  }
  const asked = [];
  await page.route('**/frontend/deploy-configs/*.json', (route) => {
    asked.push(new URL(route.request().url()).pathname);
    route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({ apiBase: API_BASE, cognitoDomain: standIn.url, clientId: CLIENT_ID }),
    });
  });
  await page.goto(`${root}${query}`);
  return asked;
}

test('the hosted page signs in through its own deployment and returns to /', async ({ page }) => {
  const hosted = await startHostedServer('e2e');
  try {
    const asked = await openHosted(page, 'e2e', '', hosted.url);
    await expect(status(page)).toHaveText('Sign in to upload your export.');
    await expect(page.locator('#devLoginField')).toBeHidden();
    expect(asked).toEqual(['/frontend/deploy-configs/e2e.json']);

    await page.click('#cognitoSignInBtn');
    await expect(status(page)).toHaveText(`Signed in as ${EMAIL}.`);
    expect(new URL(page.url()).pathname).toBe('/');
    expect(page.url()).not.toContain('code=');
  } finally {
    await page.close();
    await hosted.close();
  }
});

test('the tag wins over ?deploy=', async ({ page }) => {
  const asked = await openHosted(page, 'e2e', '?deploy=other');
  await expect(status(page)).toHaveText('Sign in to upload your export.');
  expect(asked).toEqual(['/frontend/deploy-configs/e2e.json']);
});

test('the tag is not remembered for the untagged page', async ({ page }) => {
  await openHosted(page, 'e2e');
  await expect(status(page)).toHaveText('Sign in to upload your export.');
  await page.goto(PAGE_URL);
  await expect(page.locator('#devLoginField')).toBeVisible();
  await expect(page.locator('#cognitoLoginField')).toBeHidden();
});

for (const bad of ['', '../evil']) {
  test(`a tag naming no valid deployment (${JSON.stringify(bad)}) is an error, not the dev login`, async ({ page }) => {
    const asked = await openHosted(page, bad);
    await expect(status(page)).toContainText('is not a deployment name');
    await expect(page.locator('#devLoginField')).toBeHidden();
    expect(asked).toEqual([]);
  });
}
