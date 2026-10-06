// The single holder of everything the page currently knows. Every other module
// reads and writes the shared data here rather than keeping its own copy. It
// is one object because a module cannot reassign a variable another module
// exports, but it can change the fields of an exported object.
//
// The page holds no messages beyond the page of Review on screen (plan
// docs/plans/2026-10-06-load-only-what-the-page-shows.md §5): only the
// conversations' records and their sessions, with each session's flag
// counts.

export const state = {
  // Each conversation: { name, total_messages, id, untimed }, in the order
  // GET /conversations gives them. `id` joins it to its record in
  // `records`; `untimed` counts its messages with no time (§4e).
  conversations: [],
  // Each conversation's record from GET /conversations (its metadata), by
  // id; and the user's files from GET /uploads, newest first (plan
  // docs/plans/2026-10-05-screen-flow.md §8c).
  records: new Map(),
  uploads: [],
  // Every session, as core/blocks.js's toBlock makes them, in the order
  // GET /sessions gives them (by conversation, then time).
  blocks: [],
  // The user's data version the records and sessions were read at; raised
  // by the server on every upload, flag save and scan (§5c). The analyses
  // computed in the page keep their partial results under it.
  dataVersion: null,

  // Your corrections to the automatic flags, for the messages on Review's
  // page on screen only: { [messageId]: { critical, angry, caps } }, each a
  // boolean when you stated it. Anything absent falls back to the automatic
  // flag (core/flags.js).
  overrides: {},

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
