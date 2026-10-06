// Review's questions to the server, and what the page keeps between the
// parts of an answer (plan
// docs/plans/2026-10-06-load-only-what-the-page-shows.md §5b, §8c). The
// server finds the matching messages; the page keeps only the rows of the
// page on screen, how many have matched so far, and, for each page of 50,
// the cursor it starts from, so "page 7" asks the server to carry on from
// page 7's cursor without the page ever holding every match.

import { localDayBounds } from './blocks.js';

// Rows per page; the server's own page size.
export const PAGE_ROWS = 50;

// The query for a set of filters: { conversationId, range, day, flag,
// search, view, replies }. `range` is { from, to } (ISO; a session or an
// analysis point); `day` a local day 'YYYY-MM-DD' (a Calendar day), which
// wins over `range`. A blank search is no search.
export function reviewQuery({ conversationId = null, range = null, day = null, flag = 'all', search = '', view, replies = false }){
  const query = { conversation: conversationId, flag, view, replies };
  if(day !== null) Object.assign(query, localDayBounds(day), { span: 'day' });
  else if(range !== null) Object.assign(query, { from: range.from, to: range.to, span: 'range' });
  const text = search.trim();
  if(text) query.search = text;
  return query;
}

// One part's query: the filters, where to carry on (`cursor`, with the rows
// and notes matched before it), how many full rows are wanted, and whether
// to stop once they are there ('rows') or go on counting ('end').
export function partQuery(base, { cursor = null, matched = 0, notes = 0, rows, until }){
  return { ...base, cursor, matched, notes, rows, until };
}

// What the count's walk has found so far. noteWalk(part) takes each part of
// it; the rest answer from what was taken.
export function createPaging(){
  const starts = new Map([[0, null]]);
  let matched = 0;
  let notes = 0;
  let final = false;
  return {
    noteWalk(part){
      for(const start of part.page_starts) starts.set(start.page, start.cursor);
      matched = part.matched;
      notes = part.notes;
      final = part.cursor === null;
    },
    // Whether page `page`'s start is known (page 0's always is).
    hasPage: (page) => starts.has(page),
    // The cursor page `page` starts from: null for page 0.
    startOf: (page) => starts.get(page),
    // How many pages there are, or, before the walk is done, how many are
    // known to exist so far.
    pages: () => (final ? Math.max(1, Math.ceil(matched / PAGE_ROWS)) : starts.size),
    final: () => final,
    // Messages matched so far: rows, less the notes among them.
    messages: () => matched - notes,
  };
}

// "Page 2 of 5", or "Page 2 of at least 5" while the count goes on.
export function pagesLabel(page, paging){
  return `Page ${page + 1} of ${paging.final() ? '' : 'at least '}${paging.pages()}`;
}

// "120 messages", or "at least 120 messages" while the count goes on.
export function countLabel(paging){
  const n = paging.messages();
  return `${paging.final() ? '' : 'at least '}${n} message${n === 1 ? '' : 's'}`;
}
