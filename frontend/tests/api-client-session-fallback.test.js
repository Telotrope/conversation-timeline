// The session id on a page without crypto.randomUUID, which Chrome offers
// only on secure pages (https, or this machine): infra/api-client.js then
// builds a version-4 UUID from crypto.getRandomValues (plan
// docs/plans/2026-10-05-page-coverage-gaps.md). Its own file: the id is
// made once, as the module loads.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { webcrypto } from 'node:crypto';

globalThis.window = { location: { search: '' } };
globalThis.localStorage = { getItem: () => null, setItem(){} };
Object.defineProperty(globalThis, 'crypto', {
  value: { getRandomValues: (array) => webcrypto.getRandomValues(array) },
  configurable: true,
});

const { SESSION_ID } = await import('../infra/api-client.js');

test('without crypto.randomUUID the session id is still a version-4 UUID', () => {
  assert.equal(typeof globalThis.crypto.randomUUID, 'undefined');
  assert.match(SESSION_ID, /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/);
});
