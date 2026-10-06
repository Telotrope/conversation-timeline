// Draws the Calendar tab: one row per day, a colored bar for each session and
// markers for its flags. Clicking a day, a session or a marker opens the
// matching messages in the review tab.
//
// Drawn from the sessions' stored counts (plan
// docs/plans/2026-10-06-load-only-what-the-page-shows.md §5, §6): a session
// that crosses midnight shows the same flags on each day it touches. The
// drawing is a generator, one session or one day per step, so opening the
// timeline can draw it in 50 ms turns with the bar moving (§8b).

import { formatClock, formatDuration, formatMonthHeading } from '../../core/format.js';
import { localDaysTouched } from '../../core/blocks.js';
import { viewCounts, viewName } from '../../core/session-counts.js';
import { state } from '../../core/state.js';
import { escapeAttribute } from '../render/markup.js';
import { jumpToReview, jumpToReviewDay } from './review.js';

const PALETTE = ['#3C6E64','#A6752C','#7C5C8C','#4E7BA8','#B0553F','#5E8A4E','#8C6B4F','#3E6E8E','#9C5B6E','#6E7A3C'];

function colorFor(idx){ return PALETTE[idx % PALETTE.length]; }

const FLAG_ICONS = [
  ['critical', '⚑', 'critical'],
  ['angry', '!', 'angry'],
  ['caps', 'A', 'ALL-CAPS'],
];

// A session's flag icons under the current view, each opening Review on the
// session with that flag's filter.
function flagIcons(b, view){
  const counts = viewCounts(b.counts, view);
  return FLAG_ICONS.filter(([type]) => counts[type] > 0).map(([type, glyph, words]) =>
    `<span class="flag-icon ${type}" data-flag-type="${type}" title="${counts[type]} ${words} — click to review">${glyph}</span>`);
}

function barHtml(b, i, piece, view){
  const d = new Date(piece.date + 'T00:00:00');
  const dayStart = d.getTime();
  const dayLength = new Date(d.getFullYear(), d.getMonth(), d.getDate() + 1).getTime() - dayStart;
  const leftPct = ((piece.start - dayStart) / dayLength) * 100;
  const widthPct = Math.max(((piece.end - piece.start) / dayLength) * 100, 0.5);
  const conv = state.conversations[b.conv];
  const tip = `${conv.name} · ${formatClock(b.start)}–${formatClock(b.end)} · ${formatDuration(b.duration_sec)} · ${b.count} messages · click to review these messages`;
  const icons = flagIcons(b, view);
  const flagsHtml = icons.length ? `<div class="bar-flags">${icons.join('')}</div>` : '';
  return `<div class="bar" style="left:${leftPct}%; width:${widthPct}%; background:${colorFor(b.conv)};" title="${escapeAttribute(tip)}" data-block-idx="${i}">${flagsHtml}</div>`;
}

function dayRowHtml(day, bars){
  const d = new Date(day + 'T00:00:00');
  return `<div class="day-row">
      <div class="day-label clickable" data-day="${day}" title="Click to review the entire day"><span class="num">${d.getDate()}</span>${d.toLocaleDateString(undefined,{weekday:'short'})}</div>
      <div>
        <div class="track">${bars.join('')}
          <div class="grid-line q1"></div>
          <div class="grid-line q2"></div>
          <div class="grid-line q3"></div>
        </div>
      </div>
    </div>`;
}

// The Calendar's drawing, one step per session and then one per day; yields
// { done, total } steps. One entry per day a session touches: a session
// crossing midnight is drawn as a piece at the end of one row and a piece at
// the start of the next, both opening the same session.
export function* calendarSteps(){
  const view = viewName(state.showAuto, state.showUser);
  const byDay = new Map();
  const total = state.blocks.length * 2;
  let done = 0;
  for(const [i, b] of state.blocks.entries()){
    for(const piece of localDaysTouched(b)){
      if(!byDay.has(piece.date)) byDay.set(piece.date, []);
      byDay.get(piece.date).push(barHtml(b, i, piece, view));
    }
    yield { done: ++done, total };
  }
  const days = [...byDay.keys()].sort();
  const rows = [];
  let lastMonth = null;
  for(const day of days){
    const mh = formatMonthHeading(day);
    if(mh !== lastMonth){
      rows.push(`<div class="month-heading">${mh}</div>`);
      lastMonth = mh;
    }
    rows.push(dayRowHtml(day, byDay.get(day)));
    done += byDay.get(day).length;
    yield { done: Math.min(done, total), total };
  }
  document.getElementById('calendarBody').innerHTML = `
    <div class="day-row last">
      <div></div>
      <div class="track-labels"><span>12am</span><span>6am</span><span>12pm</span><span>6pm</span><span>12am</span></div>
    </div>
    ${rows.join('')}`;
}

// The whole Calendar at once, for a redraw after the switches change.
export function renderCalendar(){
  const steps = calendarSteps();
  while(!steps.next().done);
}

// Opens Review on a session's span: all its messages, or only those with
// the flag of the icon clicked.
export function reviewSession(b, flagType){
  jumpToReview({ conv: b.conv, rangeStart: new Date(b.start).getTime(), rangeEnd: new Date(b.end).getTime(), flagType });
}

// One click handler for the whole Calendar, set once by main.js: a flag
// icon, a bar, or a day's label.
export function connectCalendar(){
  document.getElementById('calendarBody').addEventListener('click', (e) => {
    const icon = e.target.closest('.bar-flags .flag-icon');
    const bar = e.target.closest('.bar');
    const day = e.target.closest('.day-label[data-day]');
    if(bar) reviewSession(state.blocks[parseInt(bar.dataset.blockIdx, 10)], icon ? icon.dataset.flagType : 'all');
    else if(day) jumpToReviewDay(day.dataset.day);
  });
}
