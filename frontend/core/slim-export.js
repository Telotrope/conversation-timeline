// Slims a conversation export before it is sent (plan
// docs/plans/2026-10-06-load-only-what-the-page-shows.md §7b). Most of an
// export is never kept: tool results, most tool calls, Claude's thinking.
// So the page keeps only what the server uses and sends that, compressed:
// a 63.5 MB export becomes about 9 MB, 2.8 MB once compressed (measured).
//
// Kept: each conversation's id, name and times; each message's id, the
// message it answers, sender, time, attachments, file references and your
// review (`_claude_timeline_user`, so a re-uploaded annotated file keeps your
// flags); its text pieces with their citations; and the tool calls that write
// or present files, in their place among the text pieces (their type, name
// and input only).
// Dropped: everything else, including tool results, other tool calls,
// thinking, each piece's own timestamps and the message-level `text` field
// (the pieces joined, with placeholders; nothing reads it).
//
// The worker that reads the file (workers/slim-stream.js) parses it one
// conversation at a time and hands each here. Nothing here touches a file,
// a stream or the page.

// Added to a `create_file` call's input when a later command names the file:
// the server's copy of the file, replayed from Claude's edits, may then be
// older than the final one (§4, C9). The server's kept_files.rs reads it.
export const MAY_HAVE_CHANGED_MARK = '_claude_timeline_may_have_changed';

const CONVERSATION_FIELDS = ['uuid', 'name', 'created_at', 'updated_at'];
const MESSAGE_FIELDS = ['uuid', 'parent_message_uuid', 'sender', 'created_at', 'attachments', 'files', '_claude_timeline_user'];
const FILE_TOOLS = new Set(['create_file', 'str_replace', 'visualize:show_widget', 'present_files']);
// The tool calls that run a command, as the server names them.
const COMMAND_TOOLS = new Set(['bash_tool', 'bash']);
// The one field of a wrapped file (this page's annotated download) other
// than its conversations.
const FORMAT_FIELD = 'claude_timeline_format_version';

// The file isn't an export the page can slim: it is sent as it is, and the
// server says what is wrong with it.
export class NotAnExport extends Error {
  constructor(message){
    super(message);
    this.name = 'NotAnExport';
  }
}

function isObject(value){
  return value !== null && typeof value === 'object' && !Array.isArray(value);
}

function pick(source, fields){
  const out = {};
  for(const field of fields) if(field in source) out[field] = source[field];
  return out;
}

function toolCall(piece){
  return isObject(piece) && piece.type === 'tool_use' && typeof piece.name === 'string' && isObject(piece.input)
    ? piece : null;
}

// The part of a path after its last '/', as the server's base_name.
function baseName(path){
  return path.slice(path.lastIndexOf('/') + 1);
}

// Marks every `create_file` call whose file a later command names, by its
// path or its name, as the server's own replay does (kept_files.rs). The
// commands are dropped by slimming, so this is done first, on the whole
// conversation. Changes the calls' inputs in place.
export function markChangedFiles(conversation){
  const created = [];
  for(const message of messagesOf(conversation)){
    for(const piece of piecesOf(message)){
      const call = toolCall(piece);
      if(!call) continue;
      if(call.name === 'create_file' && typeof call.input.path === 'string') created.push(call.input);
      if(COMMAND_TOOLS.has(call.name) && typeof call.input.command === 'string'){
        const command = call.input.command;
        for(const input of created){
          if(command.includes(input.path) || command.includes(baseName(input.path))) input[MAY_HAVE_CHANGED_MARK] = true;
        }
      }
    }
  }
}

function messagesOf(conversation){
  return Array.isArray(conversation.chat_messages) ? conversation.chat_messages.filter(isObject) : [];
}

function piecesOf(message){
  return Array.isArray(message.content) ? message.content : [];
}

// One piece as it is sent, or null when it is dropped.
function slimPiece(piece){
  if(isObject(piece) && piece.type === 'text') return pick(piece, ['type', 'text', 'citations']);
  const call = toolCall(piece);
  if(call && FILE_TOOLS.has(call.name)) return { type: call.type, name: call.name, input: call.input };
  return null;
}

function slimMessage(message){
  if(!isObject(message)) return message;
  const out = pick(message, MESSAGE_FIELDS);
  if(Array.isArray(message.content)) out.content = message.content.map(slimPiece).filter((p) => p !== null);
  else if('content' in message) out.content = message.content;
  return out;
}

// One conversation as it is sent. Fields the server must have and the file
// lacks are left missing, not made up, so the server's error names them; a
// field of the wrong kind is passed on unchanged for the same reason.
export function slimConversation(conversation){
  markChangedFiles(conversation);
  const out = pick(conversation, CONVERSATION_FIELDS);
  if(Array.isArray(conversation.chat_messages)) out.chat_messages = conversation.chat_messages.map(slimMessage);
  else if('chat_messages' in conversation) out.chat_messages = conversation.chat_messages;
  return out;
}

// What a file's first characters say it holds: 'array' (an export as
// claude.ai gives it), 'object' (this page's annotated download, which
// wraps the conversations), 'other' (not an export), or null when the text
// so far is only blank space (and a byte-order mark).
export function rootKind(text){
  const first = /[^\s﻿]/.exec(text);
  if(!first) return null;
  if(first[0] === '[') return 'array';
  if(first[0] === '{') return 'object';
  return 'other';
}

// The parser paths that reach each conversation of a file of `kind`, and,
// in a wrapped one, its format version.
export function parserPaths(kind){
  return kind === 'array' ? ['$.*'] : ['$.conversations.*', `$.${FORMAT_FIELD}`];
}

// Builds the slimmed file's text as the parser finds each value, in the same
// shape as the original: a bare array stays one, and a wrapped file stays
// wrapped, since the server treats the two differently (a wrapped file is
// already processed and keeps its reviews). Returns { add, finish,
// conversations }:
// - add(value, key, depth) takes one value the parser found (its key, and
//   how deep it sits: 1 for a direct child of the top) and returns the text
//   to append, possibly '';
// - finish() returns the closing text;
// - conversations() counts the conversations slimmed so far.
// A conversation that isn't an object, or a file holding none, throws
// NotAnExport.
export function createSlimWriter(kind){
  if(kind !== 'array' && kind !== 'object') throw new NotAnExport('the file is neither a list of conversations nor a saved timeline');
  let count = 0;
  let version;
  const opening = kind === 'array' ? '[' : '{"conversations":[';
  return {
    add(value, key, depth){
      if(kind === 'object' && depth === 1){
        if(key === FORMAT_FIELD) version = value;
        return '';
      }
      if(!isObject(value)) throw new NotAnExport(`item ${count + 1} of the file isn't a conversation`);
      count += 1;
      return (count === 1 ? opening : ',') + JSON.stringify(slimConversation(value));
    },
    finish(){
      if(count === 0) throw new NotAnExport('the file holds no conversations');
      if(kind === 'array') return ']';
      return version === undefined ? ']}' : `],"${FORMAT_FIELD}":${JSON.stringify(version)}}`;
    },
    conversations: () => count,
  };
}
