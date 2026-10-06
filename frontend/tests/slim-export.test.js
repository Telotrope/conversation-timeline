// Slimming an export before it is sent (core/slim-export.js; plan
// docs/plans/2026-10-06-load-only-what-the-page-shows.md §7b): what is kept,
// what is dropped, the "may have been changed later" mark, and the slimmed
// file's shape.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  MAY_HAVE_CHANGED_MARK, createSlimWriter, markChangedFiles, parserPaths, rootKind, slimConversation,
} from '../core/slim-export.js';

const tool = (name, input, extra = {}) => ({ type: 'tool_use', name, input, id: 'toolu_1', start_timestamp: 't', ...extra });

test('a conversation keeps its id, name and times, and drops everything else', () => {
  const out = slimConversation({
    uuid: 'c1', name: 'One', created_at: 'a', updated_at: 'b', summary: 'long', account: { uuid: 'x' }, chat_messages: [],
  });
  assert.deepEqual(out, { uuid: 'c1', name: 'One', created_at: 'a', updated_at: 'b', chat_messages: [] });
});

test('a message keeps what the server reads and drops the rest, the text field included', () => {
  const message = {
    uuid: 'm1', parent_message_uuid: 'm0', sender: 'human', created_at: 'when', updated_at: 'later',
    text: 'joined text with This block is not supported on your current device yet.',
    attachments: [{ file_name: 'a.txt', extracted_content: 'hi' }], files: [{ file_name: 'p.png' }],
    _claude_timeline_user: { caps: true }, _claude_timeline_auto: { caps: false }, index: 3,
    content: [{ type: 'text', text: 'hello', citations: [{ start_index: 0 }], start_timestamp: 't', stop_timestamp: 'u' }],
  };
  const out = slimConversation({ uuid: 'c', chat_messages: [message] }).chat_messages[0];
  assert.deepEqual(out, {
    uuid: 'm1', parent_message_uuid: 'm0', sender: 'human', created_at: 'when',
    attachments: [{ file_name: 'a.txt', extracted_content: 'hi' }], files: [{ file_name: 'p.png' }],
    _claude_timeline_user: { caps: true },
    content: [{ type: 'text', text: 'hello', citations: [{ start_index: 0 }] }],
  });
});

test('only the tool calls that write or present files are kept, with their type, name and input', () => {
  const content = [
    { type: 'thinking', thinking: 'hmm' },
    tool('web_search', { query: 'x' }),
    { type: 'tool_result', content: [{ type: 'text', text: 'pages of results' }] },
    tool('create_file', { path: '/a/f.py', file_text: 'print(1)' }),
    tool('str_replace', { path: '/a/f.py', old_str: '1', new_str: '2' }),
    tool('visualize:show_widget', { widget_code: '<p>', title: 'W' }),
    tool('present_files', { filepaths: ['/a/f.py'] }),
    { type: 'text', text: 'done' },
    null,
    { type: 'tool_use', name: 'create_file' },
  ];
  const out = slimConversation({ chat_messages: [{ uuid: 'm', content }] }).chat_messages[0].content;
  assert.deepEqual(out, [
    { type: 'tool_use', name: 'create_file', input: { path: '/a/f.py', file_text: 'print(1)' } },
    { type: 'tool_use', name: 'str_replace', input: { path: '/a/f.py', old_str: '1', new_str: '2' } },
    { type: 'tool_use', name: 'visualize:show_widget', input: { widget_code: '<p>', title: 'W' } },
    { type: 'tool_use', name: 'present_files', input: { filepaths: ['/a/f.py'] } },
    { type: 'text', text: 'done' },
  ]);
});

test('fields of the wrong kind are passed on as they are, for the server to name', () => {
  assert.deepEqual(slimConversation({ uuid: 'c', chat_messages: 'nope' }), { uuid: 'c', chat_messages: 'nope' });
  assert.deepEqual(slimConversation({ chat_messages: [7, { uuid: 'm', content: 'x' }, { uuid: 'n' }] }).chat_messages,
    [7, { uuid: 'm', content: 'x' }, { uuid: 'n' }]);
});

test('a created file is marked when a later command names its path or its name', () => {
  const byPath = tool('create_file', { path: '/home/claude/report.md', file_text: 'a' });
  const byName = tool('create_file', { path: '/home/claude/chart.py', file_text: 'b' });
  const untouched = tool('create_file', { path: '/home/claude/notes.txt', file_text: 'c' });
  const later = tool('create_file', { path: '/home/claude/late.md', file_text: 'd' });
  const conversation = { chat_messages: [
    { uuid: 'm1', content: [byPath, byName, untouched, tool('bash_tool', { command: 'echo hi' })] },
    { uuid: 'm2', content: [
      tool('bash_tool', { command: 'pandoc /home/claude/report.md -o out.docx' }),
      tool('bash', { command: 'python chart.py' }),
      tool('bash_tool', { command: 'cat late.md' }),
      tool('bash_tool', { description: 'no command' }),
      tool('create_file', { file_text: 'no path' }),
      later,
    ] },
  ] };
  markChangedFiles(conversation);
  assert.equal(byPath.input[MAY_HAVE_CHANGED_MARK], true);
  assert.equal(byName.input[MAY_HAVE_CHANGED_MARK], true);
  assert.equal(MAY_HAVE_CHANGED_MARK in untouched.input, false);
  assert.equal(MAY_HAVE_CHANGED_MARK in later.input, false, 'a command before the file was made does not mark it');
  // Slimming marks first, then keeps the mark on the call.
  const slim = slimConversation(conversation);
  assert.equal(slim.chat_messages[0].content[0].input[MAY_HAVE_CHANGED_MARK], true);
  assert.deepEqual(slim.chat_messages[0].content.map((p) => p.name), ['create_file', 'create_file', 'create_file']);
});

test("a file's first characters say what it holds", () => {
  assert.equal(rootKind(''), null);
  assert.equal(rootKind(' \n\t﻿'), null);
  assert.equal(rootKind('﻿ [ {'), 'array');
  assert.equal(rootKind('\n{"conversations"'), 'object');
  assert.equal(rootKind('x'), 'other');
  assert.deepEqual(parserPaths('array'), ['$.*']);
  assert.deepEqual(parserPaths('object'), ['$.conversations.*', '$.claude_timeline_format_version']);
});

test('a saved timeline stays wrapped, keeping its format version', () => {
  const writer = createSlimWriter('object');
  const pieces = [
    writer.add('2', 'claude_timeline_format_version', 1),
    writer.add({ uuid: 'a', extra: 1 }, 0, 2),
    writer.add({ uuid: 'b' }, 1, 2),
    writer.add('ignored', 'other_field', 1),
  ];
  assert.equal(pieces[0], '');
  assert.equal(writer.conversations(), 2);
  assert.deepEqual(JSON.parse(pieces.join('') + writer.finish()), {
    conversations: [{ uuid: 'a' }, { uuid: 'b' }], claude_timeline_format_version: '2',
  });
  const unversioned = createSlimWriter('object');
  assert.deepEqual(JSON.parse(unversioned.add({ uuid: 'a' }, 0, 2) + unversioned.finish()), { conversations: [{ uuid: 'a' }] });
});

test('a list of conversations stays a list', () => {
  const writer = createSlimWriter('array');
  const text = writer.add({ uuid: 'a', model: 'x' }, 0, 1) + writer.add({ uuid: 'b' }, 1, 1) + writer.finish();
  assert.deepEqual(JSON.parse(text), [{ uuid: 'a' }, { uuid: 'b' }]);
  assert.throws(() => createSlimWriter('array').finish(), /holds no conversations/);
});
