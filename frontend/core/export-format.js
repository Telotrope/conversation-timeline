// Turns the export text the backend sends into the page's in-memory data: a
// list of conversations, every message, just the messages you wrote, and any
// flags already embedded in the file.

// --- Reading the processed export the backend sends back ---


export const FORMAT_VERSION = '2';

export function extractMessageText(m){
  let text = '';
  (m.content || []).forEach(piece => {
    if(piece && piece.type === 'text') text += piece.text || '';
  });
  return text;
}

// Accepts either a raw Anthropic export (bare array of conversations) or a
// file previously saved by this page (wrapped with a format-version marker).
//
// Deduplicating retried messages is the backend's job and happens at upload
// time (timeline-core's dedup pass, reached through unwrap_uploaded_json), so
// there is no second pass here. In the current flow the bare-array branch is
// not reached at all: this function only ever sees GET /export's output,
// which is always {"conversations": [...]}.
function unwrapUploadedJSON(parsed){
  if(Array.isArray(parsed)){
    return { conversations: parsed, alreadyProcessed: false };
  }
  if(parsed && Array.isArray(parsed.conversations)){
    return { conversations: parsed.conversations, alreadyProcessed: true };
  }
  throw new Error('Expected either a bare array of conversations or a {conversations: [...]} object.');
}

export function parseUploadedConversations(rawText){
  const parsedJSON = JSON.parse(rawText);
  const { conversations: data, alreadyProcessed } = unwrapUploadedJSON(parsedJSON);

  const conversations = [];
  const messages = [];
  const humanMessages = [];
  const embeddedOverrides = {};

  data.forEach((c, idx) => {
    const msgs = c.chat_messages || [];
    conversations.push({ name: c.name || '(untitled)', total_messages: msgs.length });
    msgs.forEach((m, rawIndex) => {
      const ts = m.created_at;
      if(!ts) return;
      messages.push({ conv: idx, ts });
      if(m.sender === 'human'){
        const text = extractMessageText(m);
        const id = idx + '|' + ts;

        // New schema: auto-detected and user-confirmed flags are stored in
        // two entirely separate fields, so loading a file can never let an
        // automatic pass clobber something the user explicitly decided.
        const storedAuto = m._claude_timeline_auto || null;
        const storedUser = m._claude_timeline_user || null;

        // Only a review that states at least one flag counts as yours. The
        // server writes an all-blank one for every message a scan touched,
        // and counting those would report messages you never reviewed.
        if(storedUser && ['caps', 'critical', 'angry'].some(k => typeof storedUser[k] === 'boolean')){
          embeddedOverrides[id] = storedUser;
        }

        // Automatic flags are whatever the backend computed and embedded.
        // This page performs no detection of its own, and a message with no
        // stored automatic flags simply has none -- which is the normal
        // state until the user asks for a detection pass.
        const defaultCaps = !!(storedAuto && storedAuto.caps);
        const defaultCritical = !!(storedAuto && storedAuto.critical);
        const defaultAngry = !!(storedAuto && storedAuto.angry);
        const autoSource = storedAuto ? (storedAuto.source || 'heuristic') : 'none';

        humanMessages.push({
          id,
          conv: idx,
          ts,
          text,
          rawIndex,
          default_caps: defaultCaps,
          default_critical: defaultCritical,
          default_angry: defaultAngry,
          auto_source: autoSource,
        });
      }
    });
  });

  return { conversations, messages, humanMessages, embeddedOverrides, rawData: data, alreadyProcessed };
}
