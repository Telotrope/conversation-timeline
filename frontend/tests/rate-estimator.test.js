// The time-remaining estimate under the progress bar
// (ui/widgets/status-indicators.js makeRateEstimator), with a stand-in clock:
// browser tests' downloads finish within a second, before it ever gives one
// (plan docs/plans/completed/2026-10-05-page-coverage-gaps.md).

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { makeRateEstimator } from '../ui/widgets/status-indicators.js';
import { formatEta } from '../core/format.js';

function withClock(fn){
  const real = Date.now;
  let now = 1_000_000;
  Date.now = () => now;
  try{
    return fn((ms) => { now += ms; });
  } finally {
    Date.now = real;
  }
}

test('nothing is estimated until a second has passed and something has arrived', () => {
  withClock((advance) => {
    const estimate = makeRateEstimator(10_000);
    assert.equal(estimate(0, 1000), '', 'one sample');
    advance(500);
    assert.equal(estimate(100, 1000), '', 'under a second');
    advance(600);
    const stalled = makeRateEstimator(10_000);
    assert.equal(stalled(0, 1000), '');
    advance(2000);
    assert.equal(stalled(0, 1000), '', 'nothing moved');
  });
});

test('after a second, the rest is estimated from the rate so far', () => {
  withClock((advance) => {
    const estimate = makeRateEstimator(10_000);
    estimate(0, 1000);
    advance(2000);
    // 200 bytes in 2 s is 100 bytes a second; 800 left is 8 s.
    assert.equal(estimate(200, 1000), formatEta(8));
    assert.notEqual(formatEta(8), '');
  });
});
