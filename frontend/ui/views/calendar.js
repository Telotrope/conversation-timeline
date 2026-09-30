// Draws the Calendar tab: one row per day, a colored bar for each session and
// markers for flagged messages. Clicking a day, a session or a marker opens
// the matching messages in the review tab.

import { fmtClock, fmtDuration, fmtMonthHeading } from '../../core/format.js';
import { state } from '../../core/state.js';
import { escapeHtml } from '../render/markup.js';
import { jumpToReview, jumpToReviewDay } from './review.js';

const PALETTE = ['#3C6E64','#A6752C','#7C5C8C','#4E7BA8','#B0553F','#5E8A4E','#8C6B4F','#3E6E8E','#9C5B6E','#6E7A3C'];

function colorFor(idx){ return PALETTE[idx % PALETTE.length]; }

// --- Calendar view ---
export function renderCalendar(){
  const byDay = {};
  state.blocks.forEach((b, i)=>{
    b._idx = i; // stable reference back into state.blocks for click handlers
    (byDay[b.date] = byDay[b.date] || []).push(b);
  });
  const days = Object.keys(byDay).sort();

  let html = '';
  let lastMonth = null;
  days.forEach(day=>{
    const mh = fmtMonthHeading(day);
    if(mh !== lastMonth){
      html += `<div class="month-heading">${mh}</div>`;
      lastMonth = mh;
    }
    const blocks = byDay[day];
    const d = new Date(day + 'T00:00:00');

    let bars = '';
    blocks.forEach(b=>{
      const startOfDay = new Date(b.start);
      const secOfDay = startOfDay.getHours()*3600 + startOfDay.getMinutes()*60 + startOfDay.getSeconds();
      const leftPct = (secOfDay/86400)*100;
      const widthPct = Math.max((b.duration_sec/86400)*100, 0.5);
      const conv = state.conversations[b.conv];
      let tip = `${conv.name} · ${fmtClock(b.start)}–${fmtClock(b.end)} · ${fmtDuration(b.duration_sec)} · ${b.count} messages · click to review these messages`;
      const flagIcons = [];
      if(b.criticalItems.length){ flagIcons.push(`<span class="flag-icon critical" data-flag-type="critical" title="${b.criticalItems.length} critical — click to review">⚑</span>`); }
      if(b.angryItems.length){ flagIcons.push(`<span class="flag-icon angry" data-flag-type="angry" title="${b.angryItems.length} angry — click to review">!</span>`); }
      if(b.capsItems.length){ flagIcons.push(`<span class="flag-icon caps" data-flag-type="caps" title="${b.capsItems.length} ALL-CAPS — click to review">A</span>`); }
      const flagsHtml = flagIcons.length ? `<div class="bar-flags">${flagIcons.join('')}</div>` : '';
      bars += `<div class="bar" style="left:${leftPct}%; width:${widthPct}%; background:${colorFor(b.conv)};" title="${escapeHtml(tip)}" data-block-idx="${b._idx}">${flagsHtml}</div>`;
    });

    html += `<div class="day-row">
      <div class="day-label" data-day="${day}" style="cursor:pointer;" title="Click to review the entire day"><span class="num">${d.getDate()}</span>${d.toLocaleDateString(undefined,{weekday:'short'})}</div>
      <div>
        <div class="track">${bars}
          <div class="grid-line" style="left:25%;"></div>
          <div class="grid-line" style="left:50%;"></div>
          <div class="grid-line" style="left:75%;"></div>
        </div>
      </div>
    </div>`;
  });

  document.getElementById('calendarBody').innerHTML = `
    <div class="day-row" style="border-bottom:none;">
      <div></div>
      <div class="track-labels"><span>12am</span><span>6am</span><span>12pm</span><span>6pm</span><span>12am</span></div>
    </div>
    ${html}`;

  // Flag icon click: jump to Review showing the *whole session* for context,
  // with just the flagged message(s) highlighted — not filtered down to
  // only that flag type, which stripped away the surrounding conversation.
  document.querySelectorAll('.bar-flags .flag-icon').forEach(el=>{
    el.addEventListener('click', (e)=>{
      e.stopPropagation();
      const bar = el.closest('.bar');
      const b = state.blocks[parseInt(bar.dataset.blockIdx, 10)];
      const type = el.dataset.flagType;
      const items = type === 'critical' ? b.criticalItems : type === 'angry' ? b.angryItems : b.capsItems;
      jumpToReview({
        conv: b.conv,
        rangeStart: new Date(b.start).getTime(),
        rangeEnd: new Date(b.end).getTime(),
        flagType: 'all',
        highlightIds: items.map(m=>m.id),
      });
    });
  });

  // Bar click (not on a flag icon): jump to Review showing all messages in this session
  document.querySelectorAll('.bar').forEach(el=>{
    el.addEventListener('click', ()=>{
      const b = state.blocks[parseInt(el.dataset.blockIdx, 10)];
      jumpToReview({
        conv: b.conv,
        rangeStart: new Date(b.start).getTime(),
        rangeEnd: new Date(b.end).getTime(),
        flagType: 'all',
        highlightIds: b.allHuman.map(m=>m.id),
      });
    });
  });

  // Date label click: jump straight to a whole-day view across every
  // conversation active that day.
  document.querySelectorAll('.day-label[data-day]').forEach(el=>{
    el.addEventListener('click', ()=> jumpToReviewDay(el.dataset.day));
  });
}
