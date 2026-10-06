// Small builders for the shapes the core modules read, so each test states
// only the fields it cares about.

import { toBlock } from '../core/blocks.js';
import { state } from '../core/state.js';

export function resetState() {
  Object.assign(state, {
    conversations: [], records: new Map(), uploads: [], blocks: [], dataVersion: null, overrides: {},
    showAuto: true, showUser: true, showReplies: false, selectedConversation: null, selectedAnalysis: null,
  });
}

// One message in the export's own format.
export function exportMessage(sender, text, createdAt, extra = {}) {
  return { sender, created_at: createdAt, content: [{ type: 'text', text }], ...extra };
}

// One of your messages as core/flags.js reads it (core/review-rows.js's
// messageOf makes these from Review's rows).
export function human(conv, ts, flags = {}) {
  return {
    id: `${conv}|${ts}`,
    default_caps: !!flags.caps, default_critical: !!flags.critical, default_angry: !!flags.angry,
    auto_source: 'heuristic',
  };
}

const NO_FLAGS = { caps: 0, critical: 0, angry: 0, any: 0 };

// A session's stored counts, as GET /sessions gives them: zeros for
// anything not stated.
export function counts({ messages = 0, reviewed = 0, automatic = {}, yours = {}, both = {} } = {}) {
  return {
    messages, reviewed,
    automatic: { ...NO_FLAGS, ...automatic }, yours: { ...NO_FLAGS, ...yours }, both: { ...NO_FLAGS, ...both },
  };
}

// A session as GET /sessions gives it.
export function session(conversationId, number, start, end, sessionCounts, extra = {}) {
  return {
    conversation_id: conversationId, number, start, end, placement: 'gaps',
    message_count: sessionCounts.messages, counts: sessionCounts, ...extra,
  };
}

// A session as the views draw it, in conversation `conv` (its index).
export function block(conv, number, start, end, sessionCounts, extra = {}) {
  return toBlock(session(`c${conv}`, number, start, end, sessionCounts, extra), conv);
}

// Runs an analysis's steps (or any generator) to its end, returning what it
// returns.
export function runSteps(steps) {
  for (;;) {
    const next = steps.next();
    if (next.done) return next.value;
  }
}
