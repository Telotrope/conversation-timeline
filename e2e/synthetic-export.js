// Builds a conversations export in the same shape as Anthropic's (and as
// backend/timeline-core/tests/fixtures/sample_conversations.json), for tests
// that need content the real fixture doesn't have: Markdown in a message,
// hundreds of messages, branches, citations, attachments, files, or no
// conversations at all.
//
// Each message answers the one before it (its parent_message_uuid), as a
// real export's do; the server keeps the path to the latest message and
// prunes the rest as replaced branches (plan
// docs/plans/2026-10-06-load-only-what-the-page-shows.md §4d), so messages
// that all named the conversation's start would leave only one.

const crypto = require('crypto');

const NULL_PARENT = '00000000-0000-4000-8000-000000000000';

// One message. `extra` holds what a test asks for beyond text: citations
// on the text, attachments, uploaded file names, Claude's file tool calls.
function message(m, parent) {
  const ts = m.at ? m.at.toISOString() : null;
  const reviewField = m.review ? { _claude_timeline_user: m.review } : {};
  const content = [];
  for (const call of m.toolCalls || []) {
    content.push({ type: 'tool_use', name: call.name, input: call.input, id: crypto.randomUUID() });
  }
  content.push({
    start_timestamp: ts, stop_timestamp: ts, flags: null, type: 'text', text: m.text,
    citations: m.citations || [],
  });
  const out = {
    ...reviewField,
    uuid: m.uuid || crypto.randomUUID(),
    text: m.text,
    content,
    sender: m.sender,
    updated_at: ts,
    attachments: m.attachments || [],
    files: m.files || [],
    parent_message_uuid: m.parent || parent,
  };
  if (ts) out.created_at = ts;
  return out;
}

// conversations: [{ name, uuid?, messages: [message] }], each message
// { sender: 'human'|'assistant', text, at: Date (omit for no time),
//   review?, uuid?, parent? (a uuid, to start a branch), citations?,
//   attachments?, files?, toolCalls?: [{ name, input }] }.
// review, if given, is your review of the message: { caps, critical, angry }.
// Ids are random unless given; giving them lets two exports hold the same
// conversation, as a later export repeats an earlier one's.
function syntheticExport(conversations) {
  const account = { uuid: crypto.randomUUID() };
  return JSON.stringify(conversations.map((c) => {
    let parent = NULL_PARENT;
    const msgs = c.messages.map((m) => {
      const built = message(m, parent);
      parent = built.uuid;
      return built;
    });
    const timed = msgs.filter((m) => m.created_at);
    const first = timed.length ? timed[0].created_at : new Date(0).toISOString();
    const last = timed.length ? timed[timed.length - 1].created_at : first;
    return {
      uuid: c.uuid || crypto.randomUUID(),
      name: c.name,
      summary: '',
      created_at: first,
      updated_at: last,
      account,
      chat_messages: msgs,
    };
  }));
}

module.exports = { syntheticExport, NULL_PARENT };
