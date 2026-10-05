// The deployment settings' optional pageVersion (plan
// docs/plans/completed/2026-10-02-activity-instrumentation.md §4, "Page version").

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { parseDeployConfig } from '../core/deploy-config.js';

const good = {
  apiBase: 'https://abc123.execute-api.us-east-1.amazonaws.com',
  cognitoDomain: 'https://timeline-dev-123456789012.auth.us-east-1.amazoncognito.com',
  clientId: '4hj2k3l4m5n6o7p8q9r0s1t2u3',
};
const text = (o) => JSON.stringify(o);

test('a version as git describe writes it is read as given', () => {
  for(const v of ['1a2b3c4', '1a2b3c4-dirty', 'v1.2.3-4-g1a2b3c4', 'a_b', 'x'.repeat(64)]){
    assert.equal(parseDeployConfig(text({ ...good, pageVersion: v })).pageVersion, v);
  }
});

test('absent pageVersion is left out', () => {
  assert.equal('pageVersion' in parseDeployConfig(text(good)), false);
});

test('anything else is refused, naming the field', () => {
  for(const bad of ['', 'x'.repeat(65), 'a b', 'a/b', '<script>', 7, null, true]){
    assert.throws(() => parseDeployConfig(text({ ...good, pageVersion: bad })),
      /deployment settings: pageVersion must be/, JSON.stringify(bad));
  }
});
