// Computes the three analyses on the Analytics tab that need only the
// sessions' stored counts (plan
// docs/plans/2026-10-06-load-only-what-the-page-shows.md §5c): friction
// ranking, session length against flag rate, and idle time before a
// session. Flag rate over time and time of day need each message's own
// time, so the server computes those. Nothing here draws.
//
// Each analysis is a generator that counts one session per step and yields
// how far it has got, so the Analytics view can run it in 50 ms turns
// (core/turns.js) with the bar moving. A run stopped part-way is kept, under
// its analysis, options, view and data version, and carries on from where it
// stopped when asked for again (§8c); see createAnalysisRuns.
//
// `data` is { conversations, blocks, view }: state.conversations, the
// sessions as core/blocks.js's toBlock makes them, and the view's name
// (core/session-counts.js).

import { formatDayHeading } from './format.js';
import { sessionRate } from './session-counts.js';

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

function sessionLabel(data, b){
  return `${data.conversations[b.conv].name} — ${formatDayHeading(b.date)}`;
}

function sessionPoint(data, b, rate){
  return {
    label: sessionLabel(data, b),
    conv: b.conv,
    rangeStart: new Date(b.start).getTime(),
    rangeEnd: new Date(b.end).getTime(),
    pct: ratePct(rate.flagged, rate.total),
  };
}

// --- Friction ranking ---
// By conversation, a conversation's rate adds up its sessions' counts:
// exact, since each of your messages belongs to exactly one session.
function* frictionSteps(opts, data){
  const granularity = opts.granularity || 'conversation';
  const total = data.blocks.length;
  const perConv = new Map();
  const sessionRows = [];
  for(const [i, b] of data.blocks.entries()){
    const rate = sessionRate(b.counts, data.view);
    if(rate.total > 0){
      if(granularity === 'conversation'){
        const sum = perConv.get(b.conv) || { total: 0, flagged: 0 };
        sum.total += rate.total;
        sum.flagged += rate.flagged;
        perConv.set(b.conv, sum);
      } else {
        sessionRows.push({ ...sessionPoint(data, b, rate), total: rate.total, flagged: rate.flagged });
      }
    }
    yield { done: i + 1, total };
  }
  const rows = granularity === 'conversation'
    ? [...perConv].sort(([a], [b]) => a - b).map(([conv, sum]) => ({
      label: data.conversations[conv].name,
      total: sum.total,
      flagged: sum.flagged,
      pct: ratePct(sum.flagged, sum.total),
      conv,
      rangeStart: null, rangeEnd: null,
    }))
    : sessionRows;
  rows.sort((a,b)=> b.pct - a.pct);
  return { rows, granularity };
}

// --- Session length vs. flag rate ---
function* lengthSteps(opts, data){
  const total = data.blocks.length;
  const points = [];
  for(const [i, b] of data.blocks.entries()){
    const rate = sessionRate(b.counts, data.view);
    if(rate.total > 0){
      const { pct, ...rest } = sessionPoint(data, b, rate);
      points.push({ x: b.duration_sec / 60, y: pct, ...rest });
    }
    yield { done: i + 1, total };
  }
  return { points };
}

// --- Idle time before a session ---
// For each session after the first in its conversation, the gap since the
// previous session in that conversation ended. The server gives a
// conversation's sessions in time order, so the previous one seen is the
// one before. The gap is measured from the previous session whether or not
// you reviewed it; only the later session needs counted messages for a rate.
function* idleGapSteps(opts, data){
  const total = data.blocks.length;
  const previous = new Map();
  const points = [];
  let uncountedCount = 0;
  for(const [i, b] of data.blocks.entries()){
    const prev = previous.get(b.conv);
    previous.set(b.conv, b);
    if(prev){
      const rate = sessionRate(b.counts, data.view);
      if(rate.total > 0){
        const gapHours = (new Date(b.start) - new Date(prev.end)) / 3600000;
        const { pct, ...rest } = sessionPoint(data, b, rate);
        points.push({ x: Math.max(gapHours, 0.01), y: pct, ...rest }); // the floor avoids log(0)
      } else {
        uncountedCount++;
      }
    }
    yield { done: i + 1, total };
  }
  // excludedCount: each conversation's first session, which has no prior gap.
  // uncountedCount: later sessions with no counted messages to rate.
  return { points, excludedCount: previous.size, uncountedCount };
}

const ANALYSES = Object.freeze({ friction: frictionSteps, length: lengthSteps, idlegap: idleGapSteps });

// The analyses computed here, by the names the Analytics menu uses.
export const PAGE_ANALYSES = Object.freeze(Object.keys(ANALYSES));

// The steps of one analysis, from the start.
export function analysisSteps(name, opts, data){
  return ANALYSES[name](opts, data);
}

// The runs kept so far, each { steps, finished, result }: get() returns the
// run for an analysis, its options, the view and the data version, made
// fresh the first time and the same one afterwards, so a run stopped
// part-way carries on. Runs over other sessions (a timeline loaded again)
// are forgotten.
export function createAnalysisRuns(){
  const runs = new Map();
  let blocks = null;
  return {
    get(name, opts, data, dataVersion){
      if(data.blocks !== blocks){
        runs.clear();
        blocks = data.blocks;
      }
      const key = JSON.stringify([name, opts, data.view, dataVersion]);
      if(!runs.has(key)) runs.set(key, { steps: analysisSteps(name, opts, data), finished: false, result: null });
      return runs.get(key);
    },
  };
}
