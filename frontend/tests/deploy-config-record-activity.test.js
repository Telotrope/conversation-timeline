// The deployment settings' optional recordActivity switch (plan
// docs/plans/2026-10-02-activity-instrumentation.md §4, "Turning it on and
// off"). Kept apart from deploy-config.test.js, which predates it.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { parseDeployConfig } from '../core/deploy-config.js';

const good = {
  apiBase: 'https://abc123.execute-api.us-east-1.amazonaws.com',
  cognitoDomain: 'https://timeline-dev-123456789012.auth.us-east-1.amazoncognito.com',
  clientId: '4hj2k3l4m5n6o7p8q9r0s1t2u3',
};
const text = (o) => JSON.stringify(o);

test('recordActivity true or false is read as given', () => {
  assert.equal(parseDeployConfig(text({ ...good, recordActivity: true })).recordActivity, true);
  assert.equal(parseDeployConfig(text({ ...good, recordActivity: false })).recordActivity, false);
});

test('absent recordActivity reads as not recording', () => {
  const parsed = parseDeployConfig(text(good));
  assert.equal(parsed.recordActivity === true, false);
  assert.equal('recordActivity' in parsed, false);
});

test('anything but a boolean is refused, naming the field', () => {
  for(const bad of ['true', 1, null, 'on', {}]){
    assert.throws(() => parseDeployConfig(text({ ...good, recordActivity: bad })),
      /deployment settings: recordActivity must be true or false/, JSON.stringify(bad));
  }
});
