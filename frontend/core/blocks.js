// Groups messages into sessions: a run of messages in one conversation on one
// local calendar day, split wherever 15 minutes or more pass with no
// activity. The calendar draws these as bars, and several analyses count them.

import { state } from './state.js';

const GAP_THRESHOLD_SEC = 15 * 60; // idle gaps of 15+ minutes are excluded from session blocks

// Build per-conversation, per-*local*-day session blocks from raw message
// timestamps. Bucketing happens here (client-side, in the viewer's local
// timezone) rather than being precomputed server-side in UTC, so a day's
// track always matches the same 0-24h window used to position its bars.
// Within a day, a run of messages is split into a new block whenever the
// gap since the previous message is 15 minutes or more, so idle time isn't
// counted as "active" duration.
export function localDateKey(d){
  const y = d.getFullYear();
  const m = String(d.getMonth()+1).padStart(2,'0');
  const day = String(d.getDate()).padStart(2,'0');
  return `${y}-${m}-${day}`;
}

export function buildBlocks(){
  const byConvDay = new Map();
  state.messages.forEach(m=>{
    const d = new Date(m.ts);
    const key = m.conv + '|' + localDateKey(d);
    if(!byConvDay.has(key)) byConvDay.set(key, []);
    byConvDay.get(key).push(d);
  });

  const blocks = [];
  byConvDay.forEach((dates, key) => {
    dates.sort((a,b)=>a-b);
    const [convStr, date] = key.split('|');
    const conv = parseInt(convStr, 10);

    let runStart = 0;
    for(let i=1; i<=dates.length; i++){
      const gapSec = i < dates.length ? (dates[i]-dates[i-1])/1000 : Infinity;
      if(gapSec >= GAP_THRESHOLD_SEC || i === dates.length){
        const runDates = dates.slice(runStart, i);
        const start = runDates[0];
        const end = runDates[runDates.length-1];
        blocks.push({
          conv,
          date,
          start: start.toISOString(),
          end: end.toISOString(),
          duration_sec: Math.round((end-start)/1000),
          count: runDates.length,
        });
        runStart = i;
      }
    }
  });
  return blocks;
}
