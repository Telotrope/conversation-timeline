// Signing in with Cognito, against a pretend Cognito (cognito-standin.js)
// and the local backend: page -> "Cognito" -> back with a code -> tokens ->
// upload -> timeline. See the migration plan's §V2e, E5. Real Cognito is
// deployment check D3.
//
// The page is pointed at a deployment named "e2e" (?deploy=e2e); its
// settings file is answered here, so nothing is written to disk.

const path = require('path');
const { test, expect } = require('@playwright/test');
const { failOnPageErrors } = require('./page-health');
const { startCognitoStandIn } = require('./cognito-standin');
const { API_BASE, PAGE_URL } = require('./test-endpoints');

const FIXTURE = path.resolve(
  __dirname, '..', 'backend', 'timeline-core', 'tests', 'fixtures', 'sample_conversations.json'
);
const CLIENT_ID = 'e2eclient123';
const EMAIL = 'e2e@example.com';

let standIn;

test.beforeAll(async () => {
  standIn = await startCognitoStandIn({ clientId: CLIENT_ID, apiBase: API_BASE, sub: 'cognito-e2e', email: EMAIL });
});
test.afterAll(async () => { await standIn.close(); });

test.beforeEach(async () => {
  const res = await fetch(`${API_BASE}/_dev/reset`, { method: 'POST' });
  if (!res.ok) throw new Error(`could not reset the backend: ${res.status}`);
});

failOnPageErrors();

async function openDeployed(page, settings = { apiBase: API_BASE, cognitoDomain: standIn.url, clientId: CLIENT_ID }) {
  await page.route('**/frontend/deploy-configs/e2e.json', (route) => route.fulfill({
    contentType: 'application/json',
    body: JSON.stringify(settings),
  }));
  await page.goto(`${PAGE_URL}?deploy=e2e`);
}

const status = (page) => page.locator('#cognitoLoginStatus');

test('signing in through Cognito, then uploading, then reloading', async ({ page }) => {
  await openDeployed(page);
  await expect(page.locator('#devLoginField')).toBeHidden();
  await expect(status(page)).toHaveText('Sign in to upload your export.');

  await page.click('#cognitoSignInBtn');
  await expect(status(page)).toHaveText(`Signed in as ${EMAIL}.`);
  // The one-time code is gone from the address bar.
  expect(page.url()).not.toContain('code=');
  expect(page.url()).not.toContain('state=');

  await page.setInputFiles('#loadConvFile', FIXTURE);
  await page.click('#loadBtn');
  await expect(page.locator('#mainContent')).toBeVisible({ timeout: 30_000 });

  // Same tab: still signed in, and the session comes back.
  await page.reload();
  await expect(page.locator('#restoredNotice')).toBeVisible({ timeout: 30_000 });
  await expect(page.locator('#restoredNoticeText')).toContainText(EMAIL);
});

test('a wrong PKCE proof is refused and the page says so', async ({ page }) => {
  await openDeployed(page);
  standIn.breakNextProof();
  await page.click('#cognitoSignInBtn');
  await expect(status(page)).toContainText('invalid_grant');
  await expect(page.locator('#cognitoSignInBtn')).toBeVisible();
  expect(page.url()).not.toContain('code=');
});

test('uploading before signing in asks you to sign in', async ({ page }) => {
  await openDeployed(page);
  await page.setInputFiles('#loadConvFile', FIXTURE);
  await page.click('#loadBtn');
  await expect(page.locator('#loadStatus')).toContainText('sign in first');
});

test('a deployment whose settings are missing is an error, not the dev login', async ({ page }) => {
  // No settings answered: the static server has no such file.
  await page.goto(`${PAGE_URL}?deploy=missing-deployment`);
  await expect(status(page)).toContainText('write-deploy-config.sh missing-deployment');
  await expect(page.locator('#devLoginField')).toBeHidden();
  await page.setInputFiles('#loadConvFile', FIXTURE);
  await page.click('#loadBtn');
  await expect(page.locator('#loadStatus')).toContainText('could not read frontend/deploy-configs/missing-deployment.json');
});

test('?deploy= with no name goes back to local development', async ({ page }) => {
  await openDeployed(page);
  await expect(page.locator('#cognitoLoginField')).toBeVisible();
  await page.goto(`${PAGE_URL}?deploy=`);
  await expect(page.locator('#devLoginField')).toBeVisible();
  await expect(page.locator('#cognitoLoginField')).toBeHidden();
});
