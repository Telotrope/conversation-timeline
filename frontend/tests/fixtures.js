// Small builders for the shapes the core modules read, so each test states
// only the fields it cares about.

import { state } from '../core/state.js';

export function resetState() {
  Object.assign(state, {
    conversations: [], rawData: null, messages: [], humanMessages: [], humanById: new Map(),
    blocks: [], overrides: {}, showAuto: true, showUser: true, showReplies: false,
    selectedConversation: null, selectedAnalysis: null,
  });
}

// One message in the export's own format.
export function exportMessage(sender, text, createdAt, extra = {}) {
  return { sender, created_at: createdAt, content: [{ type: 'text', text }], ...extra };
}

// A human message as parseUploadedConversations produces it.
export function human(conv, ts, flags = {}, extra = {}) {
  return {
    id: `${conv}|${ts}`, conv, ts, text: extra.text ?? 'hello', rawIndex: extra.rawIndex ?? 0,
    default_caps: !!flags.caps, default_critical: !!flags.critical, default_angry: !!flags.angry,
    auto_source: 'heuristic',
  };
}

// A synchronous stand-in for the Analytics view's chunked loop.
export const runChunked = async (items, fn) => items.map(fn);
