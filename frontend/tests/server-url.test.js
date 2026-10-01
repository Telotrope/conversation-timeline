import { test } from 'node:test';
import assert from 'node:assert/strict';
import { resolveUrl } from '../core/server-url.js';

const BASE = 'http://127.0.0.1:3000';

test('a complete address, such as an S3 presigned one, is used as it is', () => {
  const s3 = 'https://timeline-uploads-dev-1.s3.us-east-1.amazonaws.com/raw/a/b.json?X-Amz-Signature=abc';
  assert.equal(resolveUrl(BASE, s3), s3);
  assert.equal(resolveUrl(BASE, 'http://127.0.0.1:9000/bucket/key'), 'http://127.0.0.1:9000/bucket/key');
});

test("the local server's own paths get the API's address in front", () => {
  assert.equal(resolveUrl(BASE, '/_dev/local-storage/put/raw/a/b.json'), `${BASE}/_dev/local-storage/put/raw/a/b.json`);
});

test('anything else is refused, naming what was sent', () => {
  assert.throws(() => resolveUrl(BASE, 'memory://raw/a'), /memory:\/\/raw\/a/);
  assert.throws(() => resolveUrl(BASE, ''), /can't use/);
});
