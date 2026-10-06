// Citation markers placed in a reply's raw text (core/citations.js; plan
// docs/plans/2026-10-06-load-only-what-the-page-shows.md §4c).

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { MARK, MARK_CLOSE, MARK_OPEN, isWebAddress, withCitationMarks } from '../core/citations.js';

const web = (address) => ({ kind: 'web', address });
const marker = (n) => `${MARK_OPEN}${n}${MARK_CLOSE}`;

test('each cited span gets a numbered marker after it, numbered in the order the spans start', () => {
  const text = 'Paris is big. Rome is old.';
  const { text: marked, sources } = withCitationMarks(text, [
    { start: 14, end: 26, address: web('https://rome.example') },
    { start: 0, end: 13, address: web('https://paris.example') },
  ]);
  assert.equal(marked, `Paris is big.${marker(1)} Rome is old.${marker(2)}`);
  assert.deepEqual(sources, [{ number: 1, address: web('https://paris.example') }, { number: 2, address: web('https://rome.example') }]);
  assert.deepEqual([...marked.matchAll(MARK)].map((m) => m[1]), ['1', '2']);
});

test('two citations ending at the same place keep their order; positions past the end are the end', () => {
  const { text } = withCitationMarks('abc', [
    { start: 0, end: 3, address: web('https://a') },
    { start: 1, end: 3, address: web('https://b') },
    { start: 2, end: 99, address: web('https://c') },
    { start: 0, end: -4, address: web('https://d') },
  ]);
  // By start: d (0, before the start) is 1, a is 2, b is 3, c is 4; a, b
  // and c all end at the end, in that order.
  assert.equal(text, `${marker(1)}abc${marker(2)}${marker(3)}${marker(4)}`);
});

test('marker characters already in the text cannot pose as markers', () => {
  const { text, sources } = withCitationMarks(`x${MARK_OPEN}9${MARK_CLOSE}`, []);
  assert.equal(text, 'x�9�');
  assert.deepEqual(sources, []);
  assert.equal([...text.matchAll(MARK)].length, 0);
});

test('only http and https addresses the server marked as web become links', () => {
  assert.equal(isWebAddress(web('https://a.example')), true);
  assert.equal(isWebAddress(web('HTTP://a.example')), true);
  assert.equal(isWebAddress(web('javascript:alert(1)')), false);
  assert.equal(isWebAddress({ kind: 'other', address: 'https://a.example' }), false);
});
