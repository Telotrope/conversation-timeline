// The single holder of everything the page currently knows. Every other module
// reads and writes the shared data here rather than keeping its own copy. It
// is one object because a module cannot reassign a variable another module
// exports, but it can change the fields of an exported object.

export const state = {
  // Each conversation: { name, total_messages, id, untimed }. `id` joins it
  // to its record in `records`; `untimed` counts its messages with no time.
  conversations: [],
  // Each conversation's record from GET /conversations (its metadata), by
  // id; and the user's files from GET /uploads, newest first (plan
  // docs/plans/2026-10-05-screen-flow.md §8c).
  records: new Map(),
  uploads: [],
  rawData: null, // the parsed conversations.json array, kept as-is so we can re-export it annotated
  messages: [],
  humanMessages: [],
  humanById: new Map(),
  blocks: [],

  // Your manual corrections to the auto-detected flags.
  // Stored as { [messageId]: { critical: true/false, angry: true/false, caps: true/false } }
  // Only keys you've actually touched appear here; anything absent falls back
  // to the auto-detected default.
  overrides: {},

  // One handle per message of yours, from the server's GET /export reply:
  // { [serverMessageId]: handle }. A flag save must send the message's
  // handle back, which proves to the server the message is real. Replaced
  // whenever an export is loaded; null before the first one.
  flagHandles: null,

  // Global visibility switches (session-only UI state, not saved to file).
  // These affect the *effective* value of every flag everywhere: Calendar,
  // Conversations, and Review all read through this, so counts/icons stay
  // consistent with whatever the switches currently show.
  showAuto: true,
  showUser: true,
  showReplies: false,

  // Where you are: the open conversation's index and the shown analysis's
  // name, or null. The views set them; the web address is written from them.
  selectedConversation: null,
  selectedAnalysis: null,
};
