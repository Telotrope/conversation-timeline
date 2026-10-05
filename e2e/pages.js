// The two steps the page flow added around an upload (plan
// docs/plans/2026-10-05-screen-flow.md §3, §10b B1): signing in on the
// Sign-in page before the Upload page, and pressing Done on the Describe
// page after it. Shared by every spec that uploads.

const { expect } = require('./fixtures');

// Local development's sign-in: types `sub` (when given; otherwise the
// field's default stays) and presses Continue, then makes sure the Upload
// page is showing. A browser that is still signed in from an earlier visit
// skips the Sign-in page; a name that already has conversations opens the
// timeline, so this waits for it and presses "Add conversations".
async function signInToUpload(page, sub) {
  const signIn = page.locator('#signInPage');
  const upload = page.locator('#uploadPage');
  // Whichever page shows first; only one is ever visible.
  await expect(page.locator('#signInPage:visible, #uploadPage:visible, #mainContent:visible').first())
    .toBeVisible({ timeout: 30_000 });
  if (await signIn.isVisible()) {
    if (sub !== undefined) await page.fill('#devLoginSub', sub);
    await page.click('#devLoginBtn');
    await expect(page.locator('#uploadPage:visible, #mainContent:visible').first()).toBeVisible({ timeout: 30_000 });
  }
  if (await upload.isVisible()) return;
  await expect(page.locator('#loadingModal')).toBeHidden({ timeout: 30_000 });
  await page.click('#addConversationsBtn');
  await expect(upload).toBeVisible();
}

// The Describe page after an upload: accepts the answers as they are, then
// waits for the timeline to be drawn. The timeline shows as soon as its
// loading modal opens in front of it, before its data arrives, so waiting
// for the timeline alone would let a test count an empty calendar; this
// waits for the modal to close.
async function finishDescribe(page) {
  await expect(page.locator('#describePage')).toBeVisible({ timeout: 30_000 });
  await expect(page.locator('#describeBody .describe-section, #describeBody .hint').first()).toBeVisible({ timeout: 30_000 });
  await page.click('#describeSaveBtn');
  await expect(page.locator('#describePage')).toBeHidden({ timeout: 30_000 });
  await expect(page.locator('#loadingModal')).toBeHidden({ timeout: 60_000 });
}

module.exports = { signInToUpload, finishDescribe };
