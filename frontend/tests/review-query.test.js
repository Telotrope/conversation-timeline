// Review's questions to the server and what the page keeps between parts
// (core/review-query.js; plan
// docs/plans/2026-10-06-load-only-what-the-page-shows.md §5b, §8c).

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { PAGE_ROWS, countLabel, createPaging, pagesLabel, partQuery, reviewQuery } from '../core/review-query.js';

test('the filters become the query: a day wins over a range, a blank search is none', () => {
  assert.deepEqual(reviewQuery({ view: 'both' }), { conversation: null, flag: 'all', view: 'both', replies: false });
  assert.deepEqual(reviewQuery({
    conversationId: 'c1', range: { from: 'a', to: 'b' }, flag: 'caps', search: '  Shout ', view: 'yours', replies: true,
  }), { conversation: 'c1', from: 'a', to: 'b', span: 'range', flag: 'caps', search: 'Shout', view: 'yours', replies: true });
  assert.deepEqual(reviewQuery({ range: { from: 'a', to: 'b' }, day: '2026-03-02', search: '   ', view: 'automatic' }), {
    conversation: null, flag: 'all', view: 'automatic', replies: false,
    from: '2026-03-02T00:00:00.000Z', to: '2026-03-02T23:59:59.999Z', span: 'day',
  });
});

test('a part carries on from its cursor with the rows matched before it', () => {
  assert.deepEqual(partQuery({ flag: 'all' }, { rows: 50, until: 'end' }),
    { flag: 'all', cursor: null, matched: 0, notes: 0, rows: 50, until: 'end' });
  assert.deepEqual(partQuery({ flag: 'all' }, { cursor: 'c', matched: 120, notes: 2, rows: 0, until: 'rows' }),
    { flag: 'all', cursor: 'c', matched: 120, notes: 2, rows: 0, until: 'rows' });
});

test('until the count is done the page says "at least"; then the whole count', () => {
  const paging = createPaging();
  assert.equal(PAGE_ROWS, 50);
  assert.equal(paging.hasPage(0), true);
  assert.equal(paging.startOf(0), null);
  assert.equal(paging.hasPage(1), false);
  assert.equal(pagesLabel(0, paging), 'Page 1 of at least 1');
  paging.noteWalk({ page_starts: [{ page: 1, cursor: 'p1' }], matched: 73, notes: 1, cursor: 'more' });
  assert.equal(paging.startOf(1), 'p1');
  assert.equal(pagesLabel(0, paging), 'Page 1 of at least 2');
  assert.equal(countLabel(paging), 'at least 72 messages');
  paging.noteWalk({ page_starts: [{ page: 2, cursor: 'p2' }], matched: 101, notes: 1, cursor: null });
  assert.equal(paging.final(), true);
  assert.equal(pagesLabel(1, paging), 'Page 2 of 3');
  assert.equal(countLabel(paging), '100 messages');
});

test('no matches is one empty page; one match is one message', () => {
  const paging = createPaging();
  paging.noteWalk({ page_starts: [], matched: 0, notes: 0, cursor: null });
  assert.equal(pagesLabel(0, paging), 'Page 1 of 1');
  assert.equal(countLabel(paging), '0 messages');
  paging.noteWalk({ page_starts: [], matched: 1, notes: 0, cursor: null });
  assert.equal(countLabel(paging), '1 message');
});
