// Groups messages into sessions: a run of messages in one conversation with
// no pause of 15 minutes or more. Midnight does not end a session, so sessions
// are the same whatever timezone they are viewed from; only the calendar,
// which draws each day as its own row, splits a session's drawing at midnight.

import { state } from './state.js';

const GAP_THRESHOLD_SEC = 15 * 60; // a pause this long or longer ends a session

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

// A session's `date` is the local day it started on.
export function buildBlocks(){
  const byConv = new Map();
  state.messages.forEach(m=>{
    if(!byConv.has(m.conv)) byConv.set(m.conv, []);
    byConv.get(m.conv).push(new Date(m.ts));
  });

  const blocks = [];
  byConv.forEach((dates, conv) => {
    dates.sort((a,b)=>a-b);
    let runStart = 0;
    for(let i=1; i<=dates.length; i++){
      const gapSec = i < dates.length ? (dates[i]-dates[i-1])/1000 : Infinity;
      if(gapSec >= GAP_THRESHOLD_SEC){
        const start = dates[runStart];
        const end = dates[i-1];
        blocks.push({
          conv,
          date: localDateKey(start),
          start: start.toISOString(),
          end: end.toISOString(),
          duration_sec: Math.round((end-start)/1000),
          count: i - runStart,
        });
        runStart = i;
      }
    }
  });
  return blocks;
}
