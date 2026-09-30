// The download button: saves the conversations with both the automatic flags
// and your corrections written into each message, in a file this page can
// load again.

import { FORMAT_VERSION } from '../core/export-format.js';
import { state } from '../core/state.js';
import { setSaveStatus } from './widgets/status-indicators.js';

// Writes both the auto-detected values and your confirmed overrides onto
// each message (in two separate, namespaced fields, so a future load can
// never confuse one for the other), wraps the whole thing with a format
// version marker, and downloads it. This is now the only save mechanism —
// one self-contained file carries the conversation data, the automatic
// tags, and your corrections together.
export function exportAnnotatedConversations(){
  if(!state.rawData){
    setSaveStatus('No conversation data loaded to annotate.');
    return;
  }
  // Mutate state.rawData directly rather than deep-cloning it first — for a
  // file this size, a stringify-then-reparse clone briefly needs 2-3x the
  // data's size in memory all at once (original + serialized string +
  // freshly parsed copy), which is enough to crash the tab outright on a
  // large export. There's nothing unsafe about mutating in place here:
  // we only ever add two clearly namespaced fields to human messages,
  // never remove or alter anything else, so doing it again on a later
  // export is harmless and idempotent.
  const annotated = state.rawData;
  annotated.forEach((c, convIdx) => {
    (c.chat_messages || []).forEach(m => {
      if(m.sender !== 'human') return;
      const id = convIdx + '|' + m.created_at;

      const msg = state.humanById.get(id);
      if(msg){
        m._claude_timeline_auto = {
          caps: msg.default_caps,
          angry: msg.default_angry,
          critical: msg.default_critical,
          source: msg.auto_source || 'heuristic',
        };
      }

      if(state.overrides[id] && Object.keys(state.overrides[id]).length){
        m._claude_timeline_user = state.overrides[id];
      } else {
        delete m._claude_timeline_user;
      }
    });
  });

  const wrapped = { claude_timeline_format_version: FORMAT_VERSION, conversations: annotated };

  const blob = new Blob([JSON.stringify(wrapped)], {type:'application/json'});
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = 'conversations-with-flags.json';
  a.click();
  URL.revokeObjectURL(url);
  setSaveStatus('Downloaded conversations-with-flags.json — load this file directly next time.');
}
