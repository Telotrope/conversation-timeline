// Prepares a chosen file for sending (plan
// docs/plans/2026-10-06-load-only-what-the-page-shows.md §7b, §8b): reads it
// as a stream, parses it one conversation at a time with @streamparser/json
// (MIT, vendor/streamparser-json), slims each conversation
// (core/slim-export.js) and gzip-compresses the result with the browser's
// own CompressionStream. Runs in a Web Worker (workers/prepare-upload.js), so
// the page stays free to move its bar and clock.
//
// Every step is small whatever the file's size (one piece of the file
// stream, one conversation slimmed, one piece compressed), and between steps
// the clock is checked: every half second the page is told how far it has
// got. Nothing waits on the whole file at once.
//
// Messages posted, each { kind, ... }:
//   progress    { read, size, conversations, compressed }
//   done        { blob, read, size, conversations, slimmed, compressed }
//   not_export  { reason }: the file isn't an export the page can slim; the
//               page sends it as it is and the server says what is wrong
//   failed      { reason }: reading or compressing broke; the reason names
//               the error

import { JSONParser } from '../../vendor/streamparser-json/index.js';
import { createSlimWriter, NotAnExport, parserPaths, rootKind } from '../core/slim-export.js';
import { REPORT_MS } from '../core/work-budget.js';

// file: a Blob (a chosen File is one). post(message): to the page. now: the
// clock, in milliseconds. Resolves once the last message is posted.
export async function slimFile(file, post, now = () => Date.now()){
  try{
    post({ kind: 'done', ...(await slim(file, post, now)) });
  } catch(err){
    if(err instanceof NotAnExport) post({ kind: 'not_export', reason: err.message });
    else post({ kind: 'failed', reason: `${err.name}: ${err.message}` });
  }
}

async function slim(file, post, now){
  const counts = { read: 0, size: file.size, conversations: 0, slimmed: 0, compressed: 0 };
  let lastReport = now();
  const report = () => {
    if(now() - lastReport < REPORT_MS) return;
    lastReport = now();
    post({ kind: 'progress', read: counts.read, size: counts.size, conversations: counts.conversations, compressed: counts.compressed });
  };
  const gzip = new CompressionStream('gzip');
  const input = gzip.writable.getWriter();
  const output = collect(gzip.readable, counts, report);
  const reader = file.stream().getReader();
  try{
    const parsed = await parseInto(reader, input, counts, report);
    await send(input, [parsed.writer.finish()], counts, report);
    await input.close();
  } catch(err){
    // The compressed output is unfinished and of no use; aborting the
    // compression makes its reader fail, which is expected here.
    output.catch((e) => console.info(`compression stopped after a failure: ${e.message}`));
    await input.abort(err);
    await reader.cancel();
    throw err;
  }
  return { blob: new Blob(await output), ...counts };
}

// Reads the file into the parser. The parser is made once the first
// characters say whether the file is a list of conversations or a saved
// timeline, since the two reach their conversations by different paths.
async function parseInto(reader, input, counts, report){
  const decoder = new TextDecoder();
  let head = '';
  let early = [];
  let parsed = null;
  for(;;){
    const { done, value } = await reader.read();
    if(done) break;
    counts.read += value.length;
    if(!parsed){
      head += decoder.decode(value, { stream: true });
      const kind = rootKind(head);
      early.push(value);
      if(kind === null) continue;
      parsed = makeParser(kind, counts, report);
      for(const chunk of early) write(parsed, chunk);
      early = [];
    } else {
      write(parsed, value);
    }
    await send(input, parsed.take(), counts, report);
    report();
  }
  if(!parsed) throw new NotAnExport('the file is empty');
  if(!parsed.parser.isEnded) parsed.parser.end();
  if(parsed.failure()) throw parsed.failure();
  await send(input, parsed.take(), counts, report);
  return parsed;
}

function write(parsed, chunk){
  parsed.parser.write(chunk);
  if(parsed.failure()) throw parsed.failure();
}

// A parser whose every conversation is slimmed as it is found; take()
// returns the text made since the last take.
function makeParser(kind, counts, report){
  const writer = createSlimWriter(kind);
  const parser = new JSONParser({ paths: parserPaths(kind), keepStack: false });
  let pieces = [];
  let failure = null;
  parser.onValue = ({ value, key, stack }) => {
    if(failure) return;
    try{
      pieces.push(writer.add(value, key, stack.length));
      counts.conversations = writer.conversations();
      report();
    } catch(err){
      failure = err;
    }
  };
  parser.onError = (err) => {
    if(!failure) failure = new NotAnExport(`the file isn't valid JSON: ${err.message}`);
  };
  return {
    parser,
    writer,
    failure: () => failure,
    take(){
      const taken = pieces;
      pieces = [];
      return taken;
    },
  };
}

// Hands slimmed text to the compressor, one piece at a time.
async function send(input, pieces, counts, report){
  const encoder = new TextEncoder();
  for(const piece of pieces){
    if(piece === '') continue;
    const bytes = encoder.encode(piece);
    counts.slimmed += bytes.length;
    await input.write(bytes);
    report();
  }
}

// Reads the compressor's output as it comes, counting it.
async function collect(readable, counts, report){
  const chunks = [];
  const reader = readable.getReader();
  for(;;){
    const { done, value } = await reader.read();
    if(done) return chunks;
    chunks.push(value);
    counts.compressed += value.length;
    report();
  }
}
