// The timeline's sessions as the page draws them, and the viewer's local
// days. The server cuts sessions (a run of messages in one conversation with
// no pause of 15 minutes or more, or a conversation whose messages have no
// times placed by its start and end) and counts their flags (plan
// docs/plans/2026-10-06-load-only-what-the-page-shows.md §3, §6). Which
// local day a session falls on stays here, since only the browser knows the
// viewer's time zone. Midnight does not end a session; only the calendar,
// which draws each day as its own row, splits a session's drawing there.

// The viewer's local calendar date, as 'YYYY-MM-DD'.
export function localDateKey(d){
  const y = d.getFullYear();
  const m = String(d.getMonth()+1).padStart(2,'0');
  const day = String(d.getDate()).padStart(2,'0');
  return `${y}-${m}-${day}`;
}

// Each local calendar day a session touches, in order, with the part of the
// session that falls on it: [{ date, start: Date, end: Date }]. A session
// running past midnight touches two days (or more, for a long unbroken run).
export function localDaysTouched(block){
  const start = new Date(block.start), end = new Date(block.end);
  const pieces = [];
  let dayStart = new Date(start.getFullYear(), start.getMonth(), start.getDate());
  while(dayStart <= end){
    const nextDay = new Date(dayStart.getFullYear(), dayStart.getMonth(), dayStart.getDate() + 1);
    pieces.push({
      date: localDateKey(dayStart),
      start: start > dayStart ? start : dayStart,
      end: end < nextDay ? end : nextDay,
    });
    dayStart = nextDay;
  }
  return pieces;
}

// A local day ('YYYY-MM-DD') as the instants it starts and ends, in ISO
// form, for asking the server for a Calendar day's messages (the server
// doesn't know the viewer's time zone). The end is the last millisecond
// before the next midnight: the server's span includes both its ends, and a
// message sent exactly at midnight belongs to the next day only.
export function localDayBounds(dateKey){
  const [y, m, d] = dateKey.split('-').map(Number);
  const start = new Date(y, m - 1, d);
  const next = new Date(y, m - 1, d + 1);
  return { from: start.toISOString(), to: new Date(next.getTime() - 1).toISOString() };
}

// One session from GET /sessions as the views draw it. `conv` is its
// conversation's index in state.conversations; `date` the local day it
// starts on; `count` every message in it, yours and Claude's; `counts` the
// server's flag counts (core/session-counts.js).
export function toBlock(session, conv){
  const start = new Date(session.start), end = new Date(session.end);
  return {
    conv,
    number: session.number,
    date: localDateKey(start),
    start: session.start,
    end: session.end,
    duration_sec: Math.round((end - start) / 1000),
    count: session.message_count,
    placement: session.placement,
    counts: session.counts,
  };
}
