// Opening the timeline (plan
// docs/plans/2026-10-06-load-only-what-the-page-shows.md §5, §8b): the
// conversations' records and their sessions, each received in parts with
// the bar showing how many have arrived of the total, then the Calendar and
// the Conversations tab drawn in 50 ms turns with the bar showing how far
// the drawing has got. Nothing else is loaded: Review, the files and two
// of the analyses ask the server when they are shown.

import { toBlock } from '../core/blocks.js';
import { PageError } from '../core/page-error.js';
import { joinParts, readAllParts } from '../core/parts.js';
import { state } from '../core/state.js';
import { runInTurns } from '../core/turns.js';
import { TURN_MS, clockBudget } from '../core/work-budget.js';
import { fetchConversationsPart, fetchSessionsPart, fetchUploads } from '../infra/api-client.js';
import { calendarSteps } from './views/calendar.js';
import { convListSteps, forgetConversationFiles } from './views/conversations.js';
import { renderFiles } from './views/files.js';
import { renderSubtitle } from './views/header.js';
import { markReviewStale } from './views/review.js';
import { setLoadProgressIndeterminate, showLoadMeasured } from './widgets/status-indicators.js';

// How many times the records and sessions are read again because the data
// changed between them, before giving up.
const MAX_RESTARTS = 3;

function changed(){
  setLoadProgressIndeterminate('progress.data_changed');
}

// After each part: how many of `field` have arrived, of the total.
function arrived(id, field){
  return (part, parts) => showLoadMeasured(id, {}, parts.reduce((n, p) => n + p[field].length, 0), part.total);
}

// The records, then the sessions, read at the same data version: if an
// upload or a save lands between the two, both are read again.
export async function readTimeline(token){
  for(let attempt = 0; ; attempt += 1){
    const records = await readAllParts((cursor) => fetchConversationsPart(token, cursor),
      { onPart: arrived('progress.records', 'conversations'), onRestart: changed });
    const sessions = await readAllParts((cursor) => fetchSessionsPart(token, cursor),
      { onPart: arrived('progress.sessions', 'sessions'), onRestart: changed });
    if(records.dataVersion === sessions.dataVersion){
      return {
        records: joinParts(records.parts, 'conversations'),
        sessions: joinParts(sessions.parts, 'sessions'),
        dataVersion: records.dataVersion,
      };
    }
    if(attempt >= MAX_RESTARTS) throw new PageError('your data kept changing while the timeline was read; try again in a moment', 'other');
    changed();
  }
}

// Reads the timeline and draws every view. Resolves to false, drawing
// nothing, when there are no conversations.
export async function loadTimeline(token){
  const { records, sessions, dataVersion } = await readTimeline(token);
  if(records.length === 0) return false;
  const uploads = await fetchUploads(token);
  state.records = new Map(records.map((r) => [r.conversation_id, r]));
  state.uploads = uploads;
  state.conversations = records.map((r) => ({
    name: r.name || '(untitled)', total_messages: r.message_count, id: r.conversation_id, untimed: r.untimed,
  }));
  state.dataVersion = dataVersion;
  forgetConversationFiles();
  await runInTurns(drawSteps(sessions), {
    budget: () => clockBudget(TURN_MS),
    nextTurn: () => new Promise((resolve) => setTimeout(resolve, 0)),
    onProgress: ({ done, total }) => showLoadMeasured('progress.drawing', {}, done, total),
  });
  return true;
}

// The drawing, one step at a time: each session made into what the views
// draw, then the Calendar, then the conversation list; yields
// { done, total } steps.
function* drawSteps(sessions){
  const index = new Map(state.conversations.map((c, i) => [c.id, i]));
  const total = sessions.length * 3 + state.conversations.length;
  const blocks = [];
  let done = 0;
  for(const session of sessions){
    const conv = index.get(session.conversation_id);
    if(conv === undefined) console.warn(`a session of conversation ${session.conversation_id}, which has no record, is not drawn`);
    else blocks.push(toBlock(session, conv));
    yield { done: ++done, total };
  }
  state.blocks = blocks;
  for(const p of calendarSteps()) yield { done: done + p.done, total };
  done += blocks.length * 2;
  for(const p of convListSteps(document.getElementById('convSearch').value)) yield { done: done + p.done, total };
  renderSubtitle();
  renderFiles();
  markReviewStale();
}
