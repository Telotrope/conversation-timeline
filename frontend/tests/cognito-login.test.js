// infra/cognito-login.js when the sign-in library didn't load: the error
// names the file to look for (plan docs/plans/2026-10-05-page-coverage-gaps.md).

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createCognitoLogin } from '../infra/cognito-login.js';

test('without the sign-in library, creating the login names the missing file', () => {
  globalThis.window = {};
  assert.throws(
    () => createCognitoLogin({ cognitoDomain: 'https://x.auth.us-east-1.amazoncognito.com', clientId: 'abc' }),
    /the sign-in library did not load \(vendor\/oidc-client-ts\/oidc-client-ts\.min\.js\)/,
  );
  globalThis.window = { oidc: {} };
  assert.throws(() => createCognitoLogin({}), /the sign-in library did not load/);
});
