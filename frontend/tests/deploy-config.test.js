import { test } from 'node:test';
import assert from 'node:assert/strict';
import { isDeployName, parseDeployConfig } from '../core/deploy-config.js';

const good = {
  apiBase: 'https://abc123.execute-api.us-east-1.amazonaws.com/',
  cognitoDomain: 'https://timeline-dev-123456789012.auth.us-east-1.amazoncognito.com',
  clientId: '4hj2k3l4m5n6o7p8q9r0s1t2u3',
};
const text = (o) => JSON.stringify(o);

test('a complete settings file is read, without trailing slashes', () => {
  assert.deepEqual(parseDeployConfig(text(good)), {
    apiBase: 'https://abc123.execute-api.us-east-1.amazonaws.com',
    cognitoDomain: good.cognitoDomain,
    clientId: good.clientId,
  });
});

test('plain http is accepted only for this machine', () => {
  const local = { ...good, apiBase: 'http://127.0.0.1:3123', cognitoDomain: 'http://localhost:4000' };
  assert.equal(parseDeployConfig(text(local)).apiBase, 'http://127.0.0.1:3123');
  assert.throws(() => parseDeployConfig(text({ ...good, cognitoDomain: 'http://evil.example' })),
    /cognitoDomain must use https/);
});

test('each missing field is named', () => {
  for(const field of ['apiBase', 'cognitoDomain', 'clientId']){
    const partial = { ...good };
    delete partial[field];
    assert.throws(() => parseDeployConfig(text(partial)), new RegExp(`${field} is missing`));
    assert.throws(() => parseDeployConfig(text({ ...good, [field]: '' })), new RegExp(`${field} is missing`));
  }
});

test('wrong shapes are refused with what was wrong', () => {
  assert.throws(() => parseDeployConfig('{not json'), /not JSON/);
  assert.throws(() => parseDeployConfig('[]'), /must be a JSON object/);
  assert.throws(() => parseDeployConfig('null'), /must be a JSON object/);
  assert.throws(() => parseDeployConfig(text({ ...good, apiBase: 'not an address' })), /apiBase is not a web address/);
  assert.throws(() => parseDeployConfig(text({ ...good, apiBase: 'https://x.example/?a=1' })), /must not have a query/);
  assert.throws(() => parseDeployConfig(text({ ...good, clientId: 'abc"><script>' })), /clientId has unexpected characters/);
});

test('deployment names are plain lowercase names only', () => {
  for(const ok of ['dev', 'e2e', 'staging-2']) assert.equal(isDeployName(ok), true, ok);
  for(const bad of ['', 'Dev', '../secrets', 'a/b', 'x'.repeat(33), null]) assert.equal(isDeployName(bad), false, String(bad));
});
