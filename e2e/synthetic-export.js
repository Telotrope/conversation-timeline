// Builds a conversations export in the same shape as Anthropic's (and as
// backend/timeline-core/tests/fixtures/sample_conversations.json), for tests
// that need content the real fixture doesn't have: Markdown in a message,
// hundreds of messages, or no conversations at all.

const crypto = require('crypto');

const NULL_PARENT = '00000000-0000-4000-8000-000000000000';

function message(sender, text, at, review) {
  const ts = at.toISOString();
  const reviewField = review ? { _claude_timeline_user: review } : {};
  return {
    ...reviewField,
    uuid: crypto.randomUUID(),
    text,
    content: [{
      start_timestamp: ts, stop_timestamp: ts, flags: null, type: 'text', text, citations: [],
    }],
    sender,
    created_at: ts,
    updated_at: ts,
    attachments: [],
    files: [],
    parent_message_uuid: NULL_PARENT,
  };
}

// conversations: [{ name, messages: [{ sender: 'human'|'assistant', text, at: Date, review? }] }]
// review, if given, is your review of the message: { caps, critical, angry }.
function syntheticExport(conversations) {
  const account = { uuid: crypto.randomUUID() };
  return JSON.stringify(conversations.map((c) => {
    const msgs = c.messages.map((m) => message(m.sender, m.text, m.at, m.review));
    const first = msgs.length ? msgs[0].created_at : new Date(0).toISOString();
    const last = msgs.length ? msgs[msgs.length - 1].created_at : first;
    return {
      uuid: crypto.randomUUID(),
      name: c.name,
      summary: '',
      created_at: first,
      updated_at: last,
      account,
      chat_messages: msgs,
    };
  }));
}

module.exports = { syntheticExport };
