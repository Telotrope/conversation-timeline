// The worker's pipeline (workers/slim-stream.js; plan
// docs/plans/2026-10-06-load-only-what-the-page-shows.md §7b, §8b): a file
// read as a stream, parsed one conversation at a time with the vendored
// @streamparser/json, slimmed, and gzip-compressed, reporting as it goes.
// Node has the browser's streams, Blob and CompressionStream, so the real
// pipeline runs here; only the clock is a stand-in.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { slimFile } from '../workers/slim-stream.js';
import { REPORT_MS } from '../core/work-budget.js';

// A file whose stream hands over `chunks` (strings) one at a time.
function fileOf(chunks, { failAfter = null } = {}){
  const bytes = chunks.map((c) => new TextEncoder().encode(c));
  return {
    size: bytes.reduce((n, b) => n + b.length, 0),
    stream(){
      let i = 0;
      return new ReadableStream({
        pull(controller){
          if(failAfter !== null && i === failAfter) return controller.error(new TypeError('the disk went away'));
          if(i < bytes.length) controller.enqueue(bytes[i++]);
          else controller.close();
        },
      });
    },
  };
}

async function slim(file, now){
  const posted = [];
  await slimFile(file, (m) => posted.push(m), now);
  return posted;
}

async function unzip(blob){
  const text = await new Response(blob.stream().pipeThrough(new DecompressionStream('gzip'))).text();
  return JSON.parse(text);
}

const conversation = (id) => ({ uuid: id, name: id, chat_messages: [
  { uuid: `${id}-m`, sender: 'human', created_at: '2026-01-01T00:00:00Z', text: 'joined', content: [
    { type: 'text', text: 'hi' }, { type: 'thinking', thinking: 'long' },
  ] },
] });

test('a raw export is slimmed and compressed, still a list of conversations', async () => {
  const text = JSON.stringify([conversation('a'), conversation('b')]);
  // Split mid-value, to show the parser carries on across pieces.
  const posted = await slim(fileOf([text.slice(0, 7), text.slice(7, 40), text.slice(40), '\n']));
  const done = posted.at(-1);
  assert.equal(done.kind, 'done');
  assert.equal(done.conversations, 2);
  assert.equal(done.read, text.length + 1);
  assert.equal(done.size, text.length + 1);
  assert.equal(done.compressed, done.blob.size);
  const sent = await unzip(done.blob);
  assert.deepEqual(sent.map((c) => c.uuid), ['a', 'b']);
  assert.deepEqual(sent[0].chat_messages[0], {
    uuid: 'a-m', sender: 'human', created_at: '2026-01-01T00:00:00Z', content: [{ type: 'text', text: 'hi' }],
  });
  assert.equal(done.slimmed, JSON.stringify(sent).length);
});

test('a saved timeline stays wrapped, even when its first piece is only blank space', async () => {
  const text = JSON.stringify({ claude_timeline_format_version: '2', conversations: [conversation('a')] });
  const posted = await slim(fileOf(['  \n', text]));
  const sent = await unzip(posted.at(-1).blob);
  assert.deepEqual(Object.keys(sent).sort(), ['claude_timeline_format_version', 'conversations']);
  assert.equal(sent.conversations[0].uuid, 'a');
});

test('progress is posted at most every half second, with what has been read, slimmed and compressed', async () => {
  let t = 0;
  const now = () => (t += REPORT_MS); // every reading of the clock is half a second later
  const text = JSON.stringify([conversation('a'), conversation('b'), conversation('c')]);
  const posted = await slim(fileOf([text.slice(0, 30), text.slice(30)]), now);
  const progress = posted.filter((m) => m.kind === 'progress');
  assert.ok(progress.length >= 3, `${progress.length} progress messages`);
  for(const p of progress){
    assert.deepEqual(Object.keys(p).sort(), ['compressed', 'conversations', 'kind', 'read', 'size']);
    assert.equal(p.size, text.length);
  }
  assert.ok(progress.some((p) => p.conversations > 0));
  // With a clock that doesn't move, nothing but the answer is posted.
  const still = await slim(fileOf([text]), () => 0);
  assert.deepEqual(still.map((m) => m.kind), ['done']);
});

test('a file the page cannot slim is reported as such, to be sent as it is', async () => {
  const reasons = async (chunks) => {
    const posted = await slim(fileOf(chunks));
    assert.equal(posted.length, 1);
    assert.equal(posted[0].kind, 'not_export');
    return posted[0].reason;
  };
  assert.equal(await reasons([]), 'the file is empty');
  assert.equal(await reasons(['   ']), 'the file is empty');
  assert.match(await reasons(['hello']), /neither a list of conversations nor a saved timeline/);
  assert.match(await reasons(['[{"uuid":"a"},']), /isn't valid JSON/);
  assert.match(await reasons(['[{"uuid":"a"}] x']), /isn't valid JSON/);
  assert.match(await reasons(['[{"uuid":"a"}, 7]']), /item 2 of the file isn't a conversation/);
  assert.match(await reasons(['[]']), /holds no conversations/);
  assert.match(await reasons(['{"conversations": 5}']), /holds no conversations/);
});

test('a file that stops reading fails, naming the error', async () => {
  const posted = await slim(fileOf(['[{"uuid":"a"},', '{"uuid":"b"}]'], { failAfter: 1 }));
  assert.deepEqual(posted, [{ kind: 'failed', reason: 'TypeError: the disk went away' }]);
});
