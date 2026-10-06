// Review's rows as the page reads them (plan
// docs/plans/2026-10-06-load-only-what-the-page-shows.md §5, §4d): a message
// row turned into the message core/flags.js reads, its corrections, and the
// wording of a note left where a replaced branch was pruned.

const FLAGS = ['caps', 'critical', 'angry'];

// The message core/flags.js reads, from one of GET /messages's message rows:
// its automatic flags as `default_<flag>` (false before the scan has looked
// at it), and what a flag save needs (its conversation, id and handle).
export function messageOf(row){
  const auto = row.flags.auto;
  return {
    id: row.message_id,
    conversationId: row.conversation_id,
    messageId: row.message_id,
    handle: row.handle,
    at: row.at,
    default_caps: !!(auto && auto.caps),
    default_critical: !!(auto && auto.critical),
    default_angry: !!(auto && auto.angry),
    auto_source: auto ? 'heuristic' : 'none',
  };
}

// The flags you stated on a row ({ caps?, critical?, angry? }, booleans
// only), or null when you stated none.
export function overridesOf(row){
  const stated = {};
  for(const flag of FLAGS){
    const value = row.flags.user[flag];
    if(typeof value === 'boolean') stated[flag] = value;
  }
  return Object.keys(stated).length ? stated : null;
}

// The server writes an unknown time as the zero date (§4e).
function isKnown(iso){
  return Date.parse(iso) !== 0;
}

function plural(n, word){
  return `${n} ${word}${n === 1 ? '' : 's'}`;
}

// A note's wording (the user's, 2026-10-06), as { lead, link, rest }: the
// text is lead + link + rest, and `link`, when not null, is the words that
// link to the conversation the branch was kept as. `clock(iso)` formats a
// time of day (core/format.js's formatClock).
export function noteWording(note, clock){
  const at = (iso) => (isKnown(iso) ? clock(iso) : 'an unknown time');
  const below = note.replaced_by ? ', replaced by the message below' : '';
  if(note.words_not_repeated === 0 && !note.kept_as){
    return { lead: `An earlier copy of the message below was pruned here (sent ${at(note.key.at)}).`, link: null, rest: '' };
  }
  const details = `${plural(note.messages, 'message')} (${plural(note.words_not_repeated, 'word')} not repeated below) `
    + `from ${at(note.key.at)} to ${at(note.last_at)}${below}.`;
  if(note.kept_as){
    return { lead: 'An earlier branch of this conversation was kept as ', link: 'its own conversation', rest: `: ${details}` };
  }
  return { lead: `An earlier branch of this conversation was pruned here: ${details}`, link: null, rest: '' };
}
