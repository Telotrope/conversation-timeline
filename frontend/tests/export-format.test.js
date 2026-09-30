import { test } from 'node:test';
import assert from 'node:assert/strict';
import { parseUploadedConversations, extractMessageText, FORMAT_VERSION } from '../core/export-format.js';
import { exportMessage } from './fixtures.js';

test('FORMAT_VERSION marks files this page saved', () => {
  assert.equal(FORMAT_VERSION, '2');
});

test('extractMessageText joins text pieces and skips other kinds', () => {
  const m = { content: [{ type: 'text', text: 'a' }, { type: 'tool_use' }, null, { type: 'text' }, { type: 'text', text: 'b' }] };
  assert.equal(extractMessageText(m), 'ab');
  assert.equal(extractMessageText({}), '');
});

test('a wrapped export is read as already processed', () => {
  const text = JSON.stringify({ conversations: [{ name: 'One', chat_messages: [
    exportMessage('human', 'hi', '2026-01-01T10:00:00Z'),
    exportMessage('assistant', 'hello', '2026-01-01T10:00:05Z'),
  ] }] });
  const r = parseUploadedConversations(text);
  assert.equal(r.alreadyProcessed, true);
  assert.deepEqual(r.conversations, [{ name: 'One', total_messages: 2 }]);
  assert.equal(r.messages.length, 2);
  assert.equal(r.humanMessages.length, 1);
  assert.deepEqual(r.humanMessages[0], {
    id: '0|2026-01-01T10:00:00Z', conv: 0, ts: '2026-01-01T10:00:00Z', text: 'hi', rawIndex: 0,
    default_caps: false, default_critical: false, default_angry: false, auto_source: 'none',
  });
});

test('a bare array is read as a raw export, and missing names and messages are tolerated', () => {
  const r = parseUploadedConversations(JSON.stringify([{ }, { name: 'B', chat_messages: [] }]));
  assert.equal(r.alreadyProcessed, false);
  assert.deepEqual(r.conversations, [
    { name: '(untitled)', total_messages: 0 },
    { name: 'B', total_messages: 0 },
  ]);
});

test('a message without a timestamp is counted but not placed on the timeline', () => {
  const r = parseUploadedConversations(JSON.stringify([{ name: 'A', chat_messages: [{ sender: 'human' }] }]));
  assert.equal(r.conversations[0].total_messages, 1);
  assert.equal(r.messages.length, 0);
  assert.equal(r.humanMessages.length, 0);
});

test('embedded automatic and confirmed flags are read from their separate fields', () => {
  const r = parseUploadedConversations(JSON.stringify([{ name: 'A', chat_messages: [
    exportMessage('human', 'x', '2026-01-01T10:00:00Z', {
      _claude_timeline_auto: { caps: true, critical: true, angry: true, source: 'llm' },
      _claude_timeline_user: { caps: false, critical: false, angry: false },
    }),
    exportMessage('human', 'y', '2026-01-01T10:01:00Z', {
      _claude_timeline_auto: { angry: true },
      _claude_timeline_flags: { caps: true },
    }),
  ] }]));
  const [a, b] = r.humanMessages;
  assert.equal(a.default_caps && a.default_critical && a.default_angry, true);
  assert.equal(a.auto_source, 'llm');
  assert.equal(b.auto_source, 'heuristic');
  assert.equal(b.default_angry, true);
  assert.equal(b.default_caps, false);
  assert.deepEqual(r.embeddedOverrides, {
    [a.id]: { caps: false, critical: false, angry: false },
    [b.id]: { caps: true },
  });
});

test('anything other than an array or {conversations: [...]} is rejected', () => {
  assert.throws(() => parseUploadedConversations('{"nope": 1}'), /Expected either a bare array/);
  assert.throws(() => parseUploadedConversations('null'), /Expected either a bare array/);
});
