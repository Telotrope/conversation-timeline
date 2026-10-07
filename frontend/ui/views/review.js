// Draws the "Review & flags" tab, where you check and correct flags: a paged,
// searchable, filterable table of your messages with a checkbox per flag;
// the banner explaining the current filter; and the entry points other views
// use to open it on a conversation, time span or day.
//
// The server finds the messages (plan
// docs/plans/2026-10-06-load-only-what-the-page-shows.md §5b, §8c). Opening
// Review or changing its filters starts a "walk": the first part brings the
// first page of 50 rows, and the page keeps asking, with the cursor each
// part gives, until every match has been counted, keeping only the rows on
// screen and, for each page of 50, the cursor it starts from. Until the
// count is done the page says "at least N pages". Changing page asks for
// that page from its cursor and stops once it has its 50 rows.

import { localDateKey } from '../../core/blocks.js';
import { PAGE_ROWS, countLabel, createPaging, pagesLabel, partQuery, reviewQuery } from '../../core/review-query.js';
import { effectiveFlag } from '../../core/flags.js';
import { messageOf, overridesOf } from '../../core/review-rows.js';
import { viewName } from '../../core/session-counts.js';
import { state } from '../../core/state.js';
import { ensureAuthToken, fetchMessagesPart } from '../../infra/api-client.js';
import { switchTab } from '../navigation/tabs.js';
import { escapeHtml } from '../render/markup.js';
import { reviewTableHtml } from '../render/review-table.js';
import { createProgressBar } from '../widgets/status-indicators.js';

// How many times in a row a walk starts again because the data changed,
// before it stops and says so.
const MAX_RESTARTS = 3;
// How long typing in the search box must pause before the search runs.
const SEARCH_PAUSE_MS = 300;

// --- Filters ---
let convFilter = null;    // conversation id, or null for all conversations
let rangeFilter = null;   // { start, end } in ms, or null for no time restriction
let dayFilter = null;     // 'YYYY-MM-DD' (local), or null — mutually exclusive with conv/range
let highlightIds = null;  // message ids to flash/scroll to once shown
let highlightFlag = null; // or a flag: the messages with it in effect, flashed once shown

// --- What the page keeps of the results ---
let QUERY = null;          // the filters the results are for (core/review-query.js)
let PAGING = createPaging();
let PAGE = 0;
let ROWS = [];             // the rows of the page on screen
let MESSAGES = new Map();  // their messages, by id (core/review-rows.js)
let VERSION = null;        // the data version the walk is reading
let WALK = 0;              // counts walks; an older walk's answer is dropped
let PAGE_REQUEST = 0;      // counts page requests, likewise
let BUSY = 0;              // requests under way, for the bar
let FAILED = false;        // the bar shows a failure, until the next request
let STALE = true;          // the results are not for the current data
let SEARCH_TIMER = null;

const BAR = createProgressBar({
  nodes: () => ({ fill: document.getElementById('reviewProgressFill'), label: document.getElementById('reviewProgressLabel') }),
});

// What a flag checkbox, an Approve button and a file card do, supplied
// once by main.js. The review table can't import them itself: they redraw
// every view, including this one, or live at the page's top level.
let onFlagToggled = null;   // (id, type, checked) => void
let onRowApproved = null;   // (id) => void
let openFile = () => {};    // ({ conversationId, messageId, number }) => void

export function setFlagEditHandlers(toggle, approve){
  onFlagToggled = toggle;
  onRowApproved = approve;
}

export function setFileOpener(fn){
  openFile = fn;
}

// The message on the page on screen with id `id`, or undefined.
export function reviewMessage(id){
  return MESSAGES.get(id);
}

function token(){
  return ensureAuthToken(document.getElementById('devLoginSub').value.trim());
}

function currentQuery(){
  return reviewQuery({
    conversationId: convFilter,
    range: rangeFilter && { from: new Date(rangeFilter.start).toISOString(), to: new Date(rangeFilter.end).toISOString() },
    day: dayFilter,
    flag: document.getElementById('reviewFilter').value,
    search: document.getElementById('reviewSearch').value,
    view: viewName(state.showAuto, state.showUser),
    replies: state.showReplies,
  });
}

function reviewTabShown(){
  return document.getElementById('view-review').classList.contains('active');
}

// --- Asking the server ---

function setRows(rows){
  ROWS = rows;
  MESSAGES = new Map();
  state.overrides = {};
  for(const row of rows){
    if(row.kind !== 'message') continue;
    MESSAGES.set(row.message_id, messageOf(row));
    const stated = overridesOf(row);
    if(stated) state.overrides[row.message_id] = stated;
  }
}

function busy(delta){
  BUSY += delta;
  if(delta > 0) FAILED = false;
  document.getElementById('reviewProgress').hidden = BUSY === 0 && !FAILED;
  if(BUSY === 0) BAR.stop();
}

// Starts the results over for the current filters: page 0, counted from
// the start. `label` is the bar's first words.
export function startReviewQuery(label = 'progress.searching', restarts = 0){
  QUERY = currentQuery();
  PAGING = createPaging();
  PAGE = 0;
  STALE = false;
  setRows([]);
  const walk = ++WALK;
  renderReviewTable();
  runWalk(walk, true, label, restarts);
}

// The count's walk, from the start: while page 0 is on screen and not yet
// full, each part also brings its next rows.
async function runWalk(walk, fillFirstPage, label, restarts){
  busy(1);
  BAR.working(label);
  let cursor = null, matched = 0, notes = 0, version = null;
  let collected = [];
  try{
    const t = await token();
    for(;;){
      const want = fillFirstPage && PAGE === 0 ? PAGE_ROWS - collected.length : 0;
      const part = await fetchMessagesPart(t, partQuery(QUERY, { cursor, matched, notes, rows: want, until: 'end' }));
      if(walk !== WALK) return;
      if(version === null) version = part.data_version;
      if(part.data_version !== version) return dataChanged(restarts);
      VERSION = version;
      PAGING.noteWalk(part);
      if(want > 0){
        collected = collected.concat(part.rows);
        setRows(collected);
      }
      ({ matched, notes, cursor } = part);
      BAR.measured('progress.searching', {}, part.sessions_done, part.sessions_total);
      renderReviewTable();
      if(cursor === null) break;
    }
    if(PAGE >= PAGING.pages()) goToPage(PAGING.pages() - 1);
  } catch(err){
    if(walk === WALK) failed(err);
    else console.info(`a replaced count stopped: ${err.message}`);
  } finally {
    busy(-1);
  }
}

function dataChanged(restarts){
  if(restarts >= MAX_RESTARTS) return failed(new Error('your data kept changing; try again in a moment'));
  startReviewQuery('progress.data_changed', restarts + 1);
}

function failed(err){
  console.error(err);
  FAILED = true;
  document.getElementById('reviewProgress').hidden = false;
  BAR.failed('progress.request_failed', { detail: err.message });
}

// Shows page `page`, asking from its starting cursor (`start`, by default
// the one the walk found) until it has its rows.
async function goToPage(page, start = PAGING.startOf(page)){
  PAGE = page;
  const request = ++PAGE_REQUEST;
  const walk = WALK;
  setRows([]);
  renderReviewTable();
  busy(1);
  BAR.working('progress.searching');
  let cursor = start, matched = page * PAGE_ROWS;
  let collected = [];
  try{
    const t = await token();
    for(;;){
      const part = await fetchMessagesPart(t, partQuery(QUERY, { cursor, matched, rows: PAGE_ROWS - collected.length, until: 'rows' }));
      if(request !== PAGE_REQUEST || walk !== WALK) return;
      if(VERSION !== null && part.data_version !== VERSION) return dataChanged(0);
      collected = collected.concat(part.rows);
      setRows(collected);
      BAR.measured('progress.searching', {}, part.sessions_done, part.sessions_total);
      renderReviewTable();
      if(collected.length >= PAGE_ROWS || part.cursor === null) return;
      ({ cursor, matched } = part);
    }
  } catch(err){
    if(request === PAGE_REQUEST && walk === WALK) failed(err);
    else console.info(`a replaced page request stopped: ${err.message}`);
  } finally {
    busy(-1);
  }
}

// After one of your flags is saved: the page on screen asked for again
// (its rows' flags changed) and the count walked again, since a filter on a
// flag may now match more or fewer messages. The page's starting cursor
// still holds: it names a place in the messages, not a count.
export function refreshReviewPage(){
  if(QUERY === null) return;
  const page = PAGE;
  const start = PAGING.startOf(page);
  PAGING = createPaging();
  const walk = ++WALK;
  if(page === 0 || start === undefined){
    PAGE = 0;
    runWalk(walk, true, 'progress.searching', 0);
    return;
  }
  runWalk(walk, false, 'progress.searching', 0);
  goToPage(page, start);
}

// The page on screen asked for again with the current switches (Claude's
// replies on or off), keeping the count.
export function reloadReviewPage(){
  if(QUERY === null || !reviewTabShown()) return markReviewStale();
  QUERY = currentQuery();
  goToPage(PAGE, PAGE === 0 ? null : PAGING.startOf(PAGE));
}

// The results no longer fit the data (a new timeline) or the switches: asked
// for again now if Review is on screen, otherwise when it is next shown.
export function markReviewStale(){
  STALE = true;
  WALK += 1;
  if(reviewTabShown()) startReviewQuery();
}

// The Review tab was shown: its results are asked for if they aren't yet.
export function showReviewTab(){
  if(STALE) startReviewQuery();
}

// Back to the first page of results, for a changed search or filter.
export function showFirstReviewPage(){
  startReviewQuery();
}

// A search waits for typing to pause, so each keystroke isn't a request.
export function searchChanged(){
  clearTimeout(SEARCH_TIMER);
  SEARCH_TIMER = setTimeout(showFirstReviewPage, SEARCH_PAUSE_MS);
}

// --- Opening Review from elsewhere ---

// Jump into the Review tab from elsewhere on the page (a flag icon, a
// session time span, a file, or a "chat message review" link), optionally
// restricted to one conversation (`conv`, its index) and/or one time range,
// with the flag filter `flagType`, optionally with Claude's replies shown and
// rows flashed/scrolled into view once shown: specific ones (`highlightIds`),
// or, since a session arrives without its messages' ids, those with one
// flag in effect (`highlightFlag`).
export function jumpToReview({conv=null, rangeStart=null, rangeEnd=null, flagType='all', highlightIds: ids=null, highlightFlag: flag=null, showReplies=false} = {}){
  convFilter = conv === null ? null : state.conversations[conv].id;
  rangeFilter = (rangeStart != null && rangeEnd != null) ? {start: rangeStart, end: rangeEnd} : null;
  dayFilter = null;
  highlightIds = ids;
  highlightFlag = flag;
  if(showReplies){
    state.showReplies = true;
    document.getElementById('toggleShowReplies').checked = true;
  }
  document.getElementById('reviewSearch').value = '';
  document.getElementById('reviewFilter').value = flagType;
  switchTab('review');
  startReviewQuery();
}

// Jump straight to a whole-day view across every conversation active that
// day (from clicking a date label on the Calendar tab, or "View entire day"
// from a narrower filter).
export function jumpToReviewDay(dateKey){
  dayFilter = dateKey;
  convFilter = null;
  rangeFilter = null;
  highlightIds = null;
  highlightFlag = null;
  document.getElementById('reviewSearch').value = '';
  document.getElementById('reviewFilter').value = 'all';
  switchTab('review');
  startReviewQuery();
}

function shiftReviewDay(deltaDays){
  const d = new Date(dayFilter + 'T00:00:00');
  d.setDate(d.getDate() + deltaDays);
  dayFilter = localDateKey(d);
  startReviewQuery();
}

function clearReviewFilters(){
  convFilter = null;
  rangeFilter = null;
  dayFilter = null;
  highlightIds = null;
  highlightFlag = null;
  document.getElementById('reviewFilter').value = 'all';
  startReviewQuery();
}

// --- Drawing ---

function conversationIndex(id){
  return state.conversations.findIndex((c) => c.id === id);
}

function conversationName(id){
  const idx = conversationIndex(id);
  return idx >= 0 ? state.conversations[idx].name : '(a conversation no longer here)';
}

function renderReviewFilterBanner(){
  const el = document.getElementById('reviewFilterBanner');

  if(dayFilter !== null){
    el.hidden = false;
    const d = new Date(dayFilter + 'T00:00:00');
    const label = d.toLocaleDateString(undefined, {weekday:'long', month:'long', day:'numeric', year:'numeric'});
    el.innerHTML = `
      <span>
        <button id="prevDayBtn" class="btn-secondary btn-small">◀</button>
        Day: <strong>${label}</strong>
        <button id="nextDayBtn" class="btn-secondary btn-small">▶</button>
      </span>
      <button id="clearReviewFilter" class="btn-secondary">Clear filter</button>`;
    document.getElementById('prevDayBtn').addEventListener('click', ()=> shiftReviewDay(-1));
    document.getElementById('nextDayBtn').addEventListener('click', ()=> shiftReviewDay(1));
    document.getElementById('clearReviewFilter').addEventListener('click', clearReviewFilters);
    return;
  }

  if(convFilter === null && !rangeFilter){
    el.hidden = true;
    el.innerHTML = '';
    return;
  }
  el.hidden = false;
  let label = convFilter !== null ? `Conversation: <strong>${escapeHtml(conversationName(convFilter))}</strong>` : '';
  let dayKeyForButton = null;
  if(rangeFilter){
    const s = new Date(rangeFilter.start), e = new Date(rangeFilter.end);
    dayKeyForButton = localDateKey(s);
    label += `${label ? ' · ' : ''}Time span: <strong>${s.toLocaleString(undefined,{month:'short',day:'numeric',hour:'numeric',minute:'2-digit'})} – ${e.toLocaleTimeString(undefined,{hour:'numeric',minute:'2-digit'})}</strong>`;
  }

  const buttons = [];
  if(rangeFilter && convFilter !== null){
    buttons.push(`<button id="viewEntireConvBtn" class="btn-secondary">View entire conversation</button>`);
    buttons.push(`<button id="viewEntireDayBtn" class="btn-secondary">View entire day</button>`);
  }
  buttons.push(`<button id="clearReviewFilter" class="btn-secondary">Clear filter</button>`);

  el.innerHTML = `<span>${label}</span><span class="button-group">${buttons.join('')}</span>`;
  document.getElementById('clearReviewFilter').addEventListener('click', clearReviewFilters);
  const viewConvBtn = document.getElementById('viewEntireConvBtn');
  if(viewConvBtn) viewConvBtn.addEventListener('click', ()=>{
    rangeFilter = null;
    startReviewQuery();
  });
  const viewDayBtn = document.getElementById('viewEntireDayBtn');
  if(viewDayBtn) viewDayBtn.addEventListener('click', ()=> jumpToReviewDay(dayKeyForButton));
}

function renderPagination(){
  const shown = PAGING.final() ? PAGING.pages() : Math.max(PAGING.pages(), PAGE + 1);
  const label = pagesLabel(PAGE, { final: PAGING.final, pages: () => shown });
  document.getElementById('pagination').innerHTML = `
    <button id="prevPage" ${PAGE > 0 && PAGING.hasPage(PAGE - 1) ? '' : 'disabled'}>Previous</button>
    <span>${label}</span>
    <button id="nextPage" ${PAGING.hasPage(PAGE + 1) ? '' : 'disabled'}>Next</button>`;
  document.getElementById('prevPage').addEventListener('click', ()=> goToPage(PAGE - 1));
  document.getElementById('nextPage').addEventListener('click', ()=> goToPage(PAGE + 1));
}

// Draws the page of rows on screen as it is now; asks the server nothing.
export function renderReviewTable(){
  renderReviewFilterBanner();
  document.getElementById('reviewCount').textContent = countLabel(PAGING);
  document.getElementById('reviewTable').innerHTML = reviewTableHtml(ROWS, MESSAGES, { conversationName, conversationIndex });
  if(state.showUser){
    document.querySelectorAll('.flag-checkbox input:not([disabled])').forEach(cb=>{
      cb.addEventListener('change', (e)=> onFlagToggled(e.target.dataset.id, e.target.dataset.type, e.target.checked));
    });
    document.querySelectorAll('.approve-btn').forEach(btn=>{
      btn.addEventListener('click', ()=> onRowApproved(btn.dataset.id));
    });
  }
  document.querySelectorAll('#reviewTable .file-card:not([disabled])').forEach((card) => {
    card.addEventListener('click', () => openFile({
      conversationId: card.dataset.fileConv, messageId: card.dataset.fileMsg, number: Number(card.dataset.fileNumber),
    }));
  });
  renderPagination();
  showHighlights();
}

// Flashes the rows asked for once they are on screen, then forgets them.
function showHighlights(){
  if(highlightFlag){
    // Waits for a part of the page that holds one.
    const ids = [...MESSAGES.values()]
      .filter((msg) => effectiveFlag(msg, highlightFlag))
      .map((msg) => msg.id);
    if(!ids.length) return;
    highlightFlag = null;
    highlightIds = ids;
  }
  if(!highlightIds || !highlightIds.length || ROWS.length === 0) return;
  const ids = highlightIds;
  highlightIds = null;
  requestAnimationFrame(()=>{
    let firstRow = null;
    ids.forEach(id=>{
      const row = document.querySelector(`tr[data-msg-id="${CSS.escape(id)}"], tr[data-reply-id="${CSS.escape(id)}"]`);
      if(row){
        row.classList.add('row-highlight');
        if(!firstRow) firstRow = row;
      }
    });
    if(firstRow && typeof firstRow.scrollIntoView === 'function') firstRow.scrollIntoView({behavior:'smooth', block:'center'});
  });
}
