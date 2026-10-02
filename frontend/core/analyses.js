// Computes the numbers behind the five analyses on the Analytics tab. It
// draws nothing: each analysis receives a runChunked(items, fn) that maps fn
// over items (the Analytics view passes one that yields to the browser
// between chunks) and returns the data its renderer takes.

import { countsTowardRates, isFlagged } from './flags.js';
import { formatDayHeading } from './format.js';
import { state } from './state.js';

export function pearsonR(xs, ys){
  const n = xs.length;
  if(n < 2) return null;
  const meanX = xs.reduce((a,b)=>a+b,0) / n;
  const meanY = ys.reduce((a,b)=>a+b,0) / n;
  let num = 0, denX = 0, denY = 0;
  for(let i=0;i<n;i++){
    const dx = xs[i]-meanX, dy = ys[i]-meanY;
    num += dx*dy; denX += dx*dx; denY += dy*dy;
  }
  if(denX === 0 || denY === 0) return null;
  return num / Math.sqrt(denX*denY);
}

// Percentage of counted messages that are flagged. A rate over no messages
// is meaningless, so callers leave such rows out beforehand; reaching here
// with none is a bug, and says so rather than drawing a misleading 0%.
function ratePct(flagged, total){
  if(total === 0) throw new Error('flag rate asked for with no counted messages');
  return flagged / total * 100;
}

// Your counted messages in a session, and how many of them are flagged.
// b.count is not used: it includes Claude's messages, which are never flagged.
function sessionRate(b){
  const counted = b.allHuman.filter(countsTowardRates);
  return { total: counted.length, flagged: counted.filter(isFlagged).length };
}

// --- Friction ranking ---
export async function computeFrictionAnalysis(opts, runChunked){
  const granularity = opts.granularity || 'conversation';
  let rows;

  if(granularity === 'conversation'){
    const perConv = state.conversations.map(() => ({ total: 0, flagged: 0 }));
    await runChunked(state.humanMessages, m => {
      if(!countsTowardRates(m)) return;
      perConv[m.conv].total++;
      if(isFlagged(m)) perConv[m.conv].flagged++;
    });
    rows = state.conversations
      .map((c, idx) => ({ c, idx }))
      .filter(({ idx }) => perConv[idx].total > 0)
      .map(({ c, idx }) => ({
        label: c.name,
        total: perConv[idx].total,
        flagged: perConv[idx].flagged,
        pct: ratePct(perConv[idx].flagged, perConv[idx].total),
        conv: idx,
        rangeStart: null, rangeEnd: null,
      }));
  } else {
    const counted = state.blocks.filter(b => sessionRate(b).total > 0);
    rows = await runChunked(counted, b => {
      const { total, flagged } = sessionRate(b);
      return {
        label: `${state.conversations[b.conv].name} — ${formatDayHeading(b.date)}`,
        total,
        flagged,
        pct: ratePct(flagged, total),
        conv: b.conv,
        rangeStart: new Date(b.start).getTime(),
        rangeEnd: new Date(b.end).getTime(),
      };
    });
  }

  rows.sort((a,b)=> b.pct - a.pct);
  return { rows, granularity };
}

// --- Flag rate over time ---
export async function computeTrendAnalysis(opts, runChunked){
  const granularity = opts.granularity || 'week';
  function bucketKey(d){
    if(granularity === 'month'){
      return `${d.getFullYear()}-${String(d.getMonth()+1).padStart(2,'0')}`;
    }
    // ISO-ish week bucket: year + week number (Sunday-start, matching the rest of this page)
    const first = new Date(d.getFullYear(), 0, 1);
    const dayOfYear = Math.floor((d - first) / 86400000);
    const week = Math.floor((dayOfYear + first.getDay()) / 7);
    return `${d.getFullYear()}-W${String(week).padStart(2,'0')}`;
  }

  const buckets = new Map();
  await runChunked(state.humanMessages, m => {
    if(!countsTowardRates(m)) return;
    const key = bucketKey(new Date(m.ts));
    if(!buckets.has(key)) buckets.set(key, {total:0, flagged:0});
    const b = buckets.get(key);
    b.total++;
    if(isFlagged(m)) b.flagged++;
  });

  const keys = Array.from(buckets.keys()).sort();
  const points = keys.map(k => ({
    x: k,
    y: ratePct(buckets.get(k).flagged, buckets.get(k).total),
    total: buckets.get(k).total,
    flagged: buckets.get(k).flagged,
  }));
  return { points, granularity };
}

// --- Session length vs. flag rate ---
export async function computeLengthAnalysis(opts, runChunked){
  const counted = state.blocks.filter(b => sessionRate(b).total > 0);
  const points = await runChunked(counted, b => {
    const { total, flagged } = sessionRate(b);
    return {
      x: b.duration_sec / 60, // minutes
      y: ratePct(flagged, total),
      label: `${state.conversations[b.conv].name} — ${formatDayHeading(b.date)}`,
      conv: b.conv,
      rangeStart: new Date(b.start).getTime(),
      rangeEnd: new Date(b.end).getTime(),
    };
  });
  return { points };
}

// --- Time of day & day of week ---
export async function computeTimeOfDayAnalysis(opts, runChunked){
  const byHour = Array.from({length:24}, () => ({total:0, flagged:0}));
  const byDow = Array.from({length:7}, () => ({total:0, flagged:0}));
  await runChunked(state.humanMessages, m => {
    if(!countsTowardRates(m)) return;
    const d = new Date(m.ts);
    const h = d.getHours(), dow = d.getDay();
    byHour[h].total++; byDow[dow].total++;
    if(isFlagged(m)){ byHour[h].flagged++; byDow[dow].flagged++; }
  });
  return { byHour, byDow };
}

// --- Idle time before a session ---
export async function computeIdleGapAnalysis(opts, runChunked){
  // For each session (block) after the first one in its conversation, the
  // gap since the previous session in that same conversation ended.
  const byConv = new Map();
  state.blocks.forEach(b => {
    if(!byConv.has(b.conv)) byConv.set(b.conv, []);
    byConv.get(b.conv).push(b);
  });
  byConv.forEach(list => list.sort((a,b)=> new Date(a.start) - new Date(b.start)));

  // The gap is measured from the previous session whether or not you
  // reviewed it; only the later session needs counted messages for a rate.
  const pairs = [];
  let uncountedCount = 0;
  byConv.forEach(list => {
    for(let i=1;i<list.length;i++){
      if(sessionRate(list[i]).total > 0) pairs.push({ prev: list[i-1], cur: list[i] });
      else uncountedCount++;
    }
  });

  const points = await runChunked(pairs, ({prev, cur}) => {
    const gapHours = (new Date(cur.start) - new Date(prev.end)) / 3600000;
    const { total, flagged } = sessionRate(cur);
    return {
      x: Math.max(gapHours, 0.01), // avoid log(0)
      y: ratePct(flagged, total),
      label: `${state.conversations[cur.conv].name} — ${formatDayHeading(cur.date)}`,
      conv: cur.conv,
      rangeStart: new Date(cur.start).getTime(),
      rangeEnd: new Date(cur.end).getTime(),
    };
  });

  // excludedCount: each conversation's first session, which has no prior gap.
  // uncountedCount: later sessions with no counted messages to rate.
  return { points, excludedCount: byConv.size, uncountedCount };
}
