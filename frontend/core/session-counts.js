// Reads a session's stored flag counts under the show switches (plan
// docs/plans/2026-10-06-load-only-what-the-page-shows.md §6). The server
// counts every session's messages once for each view (automatic tags only,
// yours only, both with yours winning), so the Calendar, the Conversations
// tab and three analyses never need the messages themselves.

// The four views, by the names the server uses. "neither" counts nothing.
export const VIEWS = Object.freeze(['both', 'automatic', 'yours', 'neither']);

// The view the two show switches pick.
export function viewName(showAuto, showUser){
  if(showAuto && showUser) return 'both';
  if(showAuto) return 'automatic';
  if(showUser) return 'yours';
  return 'neither';
}

const NONE = Object.freeze({ caps: 0, critical: 0, angry: 0, any: 0 });

// A session's messages with each flag under `view`, and with any of the
// three ({ caps, critical, angry, any }). `counts` is a session's stored
// counts, as GET /sessions gives them.
export function viewCounts(counts, view){
  return view === 'neither' ? NONE : counts[view];
}

// How many of a session's messages count towards a rate under `view`:
// with automatic tags shown every message of yours has a value; with only
// yours shown, only the ones you reviewed; with neither, none (counting the
// rest as "not flagged" would report unreviewed messages as clean).
export function countedMessages(counts, view){
  if(view === 'automatic' || view === 'both') return counts.messages;
  if(view === 'yours') return counts.reviewed;
  return 0;
}

// A session's rate: { total, flagged }, total being the counted messages
// and flagged those with any of the three flags.
export function sessionRate(counts, view){
  return { total: countedMessages(counts, view), flagged: viewCounts(counts, view).any };
}
