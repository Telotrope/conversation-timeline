// Signing in with Cognito, against a pretend Cognito (cognito-standin.js)
// and the local backend: page -> "Cognito" -> back with a code -> tokens ->
// upload -> timeline. See the migration plan's §V2e, E5. Real Cognito is
// deployment check D3.
//
// The page is pointed at a deployment named "e2e" (?deploy=e2e); its
// settings file is answered here, so nothing is written to disk.

const path = require('path');
const { test, expect } = require('./fixtures');
const { failOnPageErrors } = require('./page-health');
const { startCognitoStandIn } = require('./cognito-standin');
const { API_BASE, PAGE_URL } = require('./test-endpoints');
const { finishDescribe } = require('./pages');

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
  await finishDescribe(page);
  await expect(page.locator('#mainContent')).toBeVisible({ timeout: 30_000 });

  // Same tab: still signed in, and the session comes back behind the
  // loading modal (its sessions held for half a second so it can be seen).
  await page.route((url) => url.origin === API_BASE && url.pathname === '/sessions', async (route) => {
    await new Promise((resolve) => setTimeout(resolve, 500));
    await route.continue();
  });
  await page.reload();
  await expect(page.locator('#loadingModal')).toBeVisible({ timeout: 30_000 });
  await expect(page.locator('#loadingModal')).toBeHidden({ timeout: 30_000 });
  await expect(page.locator('#mainContent')).toBeVisible();
  await expect(page.locator('#accountName')).toHaveText(EMAIL);
});

test('signing out forgets the sign-in in this tab', async ({ page }) => {
  await openDeployed(page);
  await page.click('#cognitoSignInBtn');
  await expect(status(page)).toHaveText(`Signed in as ${EMAIL}.`);

  // Sign out is on the account line of every signed-in page (plan
  // 2026-10-05-screen-flow.md §5).
  await page.click('#accountSignOutBtn');
  await expect(page.locator('#signInPage')).toBeVisible();
  await expect(status(page)).toHaveText('Sign in to upload your export.');
  await expect(page.locator('#cognitoSignInBtn')).toBeVisible();
  await expect(page.locator('#cognitoSignOutBtn')).toBeHidden();
});

test('a sign-in that fails to start says so and stays on the page', async ({ page }) => {
  await openDeployed(page);
  await expect(status(page)).toHaveText('Sign in to upload your export.');
  // Stands in for the sign-in library failing before it leaves the page
  // (plan docs/plans/completed/2026-10-05-page-coverage-gaps.md).
  await page.evaluate(() => {
    window.oidc.UserManager.prototype.signinRedirect = () => Promise.reject(new Error('stand-in failure'));
  });
  const before = page.url();
  await page.click('#cognitoSignInBtn');
  await expect(status(page)).toHaveText('Could not start signing in: stand-in failure');
  expect(page.url()).toBe(before);
});

test('a wrong PKCE proof is refused and the page says so', async ({ page }) => {
  await openDeployed(page);
  standIn.breakNextProof();
  await page.click('#cognitoSignInBtn');
  await expect(status(page)).toContainText('invalid_grant');
  await expect(page.locator('#cognitoSignInBtn')).toBeVisible();
  expect(page.url()).not.toContain('code=');
});

test('signed out, the page shows Sign-in, and the Upload page can\'t be reached', async ({ page }) => {
  await openDeployed(page);
  await expect(page.locator('#signInPage')).toBeVisible();
  await expect(page.locator('#uploadPage')).toBeHidden();
  // Asking for the Upload page by its address still shows Sign-in.
  await page.goto(`${PAGE_URL}?deploy=e2e#upload`);
  await expect(page.locator('#signInPage')).toBeVisible();
  await expect(page.locator('#uploadPage')).toBeHidden();
  expect(new URL(page.url()).hash).toBe('#signin');
});

test('a deployment whose settings are missing is an error, not the dev login', async ({ page }) => {
  // No settings answered: the static server has no such file.
  await page.goto(`${PAGE_URL}?deploy=missing-deployment`);
  await expect(status(page)).toContainText('write-deploy-config.sh missing-deployment');
  await expect(page.locator('#devLoginField')).toBeHidden();
  // With no working sign-in, nothing past the Sign-in page can be reached.
  await expect(page.locator('#signInPage')).toBeVisible();
  await expect(page.locator('#uploadPage')).toBeHidden();
});

test('?deploy= with no name goes back to local development', async ({ page }) => {
  await openDeployed(page);
  await expect(page.locator('#cognitoLoginField')).toBeVisible();
  await page.goto(`${PAGE_URL}?deploy=`);
  await expect(page.locator('#devLoginField')).toBeVisible();
  await expect(page.locator('#cognitoLoginField')).toBeHidden();
});

test('a sign-in that runs out sends you back to Sign-in, saying so', async ({ page }) => {
  await openDeployed(page);
  await page.click('#cognitoSignInBtn');
  await expect(page.locator('#uploadPage')).toBeVisible({ timeout: 30_000 });
  // Cognito's sign-in lasts an hour; this one has run out.
  await page.evaluate(() => {
    const key = Object.keys(sessionStorage).find((k) => k.startsWith('oidc.user:'));
    const user = JSON.parse(sessionStorage.getItem(key));
    user.expires_at = Math.floor(Date.now() / 1000) - 60;
    sessionStorage.setItem(key, JSON.stringify(user));
  });
  await page.setInputFiles('#loadConvFile', FIXTURE);
  await page.click('#loadBtn');
  await expect(page.locator('#signInPage')).toBeVisible();
  await expect(page.locator('#signInStatus')).toHaveText('Your sign-in ran out; sign in again.');
});

// Marks the stored Cognito sign-in as run out.
async function expireSignIn(page) {
  await page.evaluate(() => {
    const key = Object.keys(sessionStorage).find((k) => k.startsWith('oidc.user:'));
    const user = JSON.parse(sessionStorage.getItem(key));
    user.expires_at = Math.floor(Date.now() / 1000) - 60;
    sessionStorage.setItem(key, JSON.stringify(user));
  });
}

test('a sign-in that runs out on Describe, then on the way to the timeline, returns to Sign-in', async ({ page }) => {
  await openDeployed(page);
  await page.click('#cognitoSignInBtn');
  await expect(page.locator('#uploadPage')).toBeVisible({ timeout: 30_000 });
  await page.setInputFiles('#loadConvFile', FIXTURE);
  await page.click('#loadBtn');
  await expect(page.locator('#describeBody .describe-section')).toBeVisible({ timeout: 30_000 });
  await expireSignIn(page);
  // Done needs the server; so does Cancel, which opens the timeline.
  await page.click('#describeSaveBtn');
  await expect(page.locator('#signInStatus')).toHaveText('Your sign-in ran out; sign in again.');

  await page.click('#cognitoSignInBtn');
  await expect(page.locator('#loadingModal')).toBeHidden({ timeout: 30_000 });
  await expect(page.locator('#mainContent')).toBeVisible();
  await page.click('#addConversationsBtn');
  await page.setInputFiles('#loadConvFile', FIXTURE);
  await page.click('#loadBtn');
  await expect(page.locator('#describePage')).toBeVisible({ timeout: 30_000 });
  await expireSignIn(page);
  await page.click('#describeLeaveBtn');
  await expect(page.locator('#signInPage')).toBeVisible();
  await expect(page.locator('#signInStatus')).toHaveText('Your sign-in ran out; sign in again.');
});
