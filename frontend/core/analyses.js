// Computes the numbers behind the five analyses on the Analytics tab. It
// draws nothing: each analysis receives a runChunked(items, fn) that maps fn
// over items (the Analytics view passes one that yields to the browser
// between chunks) and returns the data its renderer takes.

import { isFlagged } from './flags.js';
import { fmtDayHeading } from './format.js';
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

// --- Friction ranking ---
export async function computeFrictionAnalysis(opts, runChunked){
  const granularity = opts.granularity || 'conversation';
  let rows;

  if(granularity === 'conversation'){
    const perConv = state.conversations.map(() => ({ total: 0, flagged: 0 }));
    await runChunked(state.humanMessages, m => {
      perConv[m.conv].total++;
      if(isFlagged(m)) perConv[m.conv].flagged++;
    });
    rows = state.conversations.map((c, idx) => ({
      label: c.name,
      total: perConv[idx].total,
      flagged: perConv[idx].flagged,
      pct: perConv[idx].total ? (perConv[idx].flagged / perConv[idx].total * 100) : 0,
      conv: idx,
      rangeStart: null, rangeEnd: null,
    })).filter(r => r.total > 0);
  } else {
    rows = await runChunked(state.blocks, b => ({
      label: `${state.conversations[b.conv].name} — ${fmtDayHeading(b.date)}`,
      total: b.count,
      flagged: b.criticalItems.length + b.angryItems.length + b.capsItems.length > 0
        ? b.allHuman.filter(isFlagged).length : 0,
      pct: b.count ? (b.allHuman.filter(isFlagged).length / b.count * 100) : 0,
      conv: b.conv,
      rangeStart: new Date(b.start).getTime(),
      rangeEnd: new Date(b.end).getTime(),
    }));
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
    const key = bucketKey(new Date(m.ts));
    if(!buckets.has(key)) buckets.set(key, {total:0, flagged:0});
    const b = buckets.get(key);
    b.total++;
    if(isFlagged(m)) b.flagged++;
  });

  const keys = Array.from(buckets.keys()).sort();
  const points = keys.map(k => ({
    x: k,
    y: buckets.get(k).total ? (buckets.get(k).flagged / buckets.get(k).total * 100) : 0,
    total: buckets.get(k).total,
    flagged: buckets.get(k).flagged,
  }));
  return { points, granularity };
}

// --- Session length vs. flag rate ---
export async function computeLengthAnalysis(opts, runChunked){
  const points = await runChunked(state.blocks.filter(b=>b.count>0), b => {
    const flaggedCount = b.allHuman.filter(isFlagged).length;
    return {
      x: b.duration_sec / 60, // minutes
      y: flaggedCount / b.count * 100,
      label: `${state.conversations[b.conv].name} — ${fmtDayHeading(b.date)}`,
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

  const pairs = [];
  byConv.forEach(list => {
    for(let i=1;i<list.length;i++){
      pairs.push({ prev: list[i-1], cur: list[i] });
    }
  });

  const points = await runChunked(pairs, ({prev, cur}) => {
    const gapHours = (new Date(cur.start) - new Date(prev.end)) / 3600000;
    const flaggedCount = cur.allHuman.filter(isFlagged).length;
    return {
      x: Math.max(gapHours, 0.01), // avoid log(0)
      y: cur.count ? (flaggedCount / cur.count * 100) : 0,
      label: `${state.conversations[cur.conv].name} — ${fmtDayHeading(cur.date)}`,
      conv: cur.conv,
      rangeStart: new Date(cur.start).getTime(),
      rangeEnd: new Date(cur.end).getTime(),
    };
  });

  return { points, excludedCount: state.blocks.length - points.length };
}
