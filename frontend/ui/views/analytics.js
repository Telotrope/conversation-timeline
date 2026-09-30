// Draws the Analytics tab: runs the chosen analysis from core/analyses.js in
// small chunks so the progress bar moves and the page stays responsive, then
// draws the result with its chart. Results link through to the review tab.

import { computeFrictionAnalysis, computeIdleGapAnalysis, computeLengthAnalysis, computeTimeOfDayAnalysis, computeTrendAnalysis, pearsonR } from '../../core/analyses.js';
import { state } from '../../core/state.js';
import { rememberLocation } from '../navigation/location.js';
import { renderBarChartSVG, renderLineChartSVG, renderScatterChartSVG } from '../render/charts.js';
import { escapeHtml } from '../render/markup.js';
import { jumpToReview } from './review.js';

// Processes `items` in chunks (yielding to the browser between chunks via
// requestAnimationFrame) so a progress bar can actually animate and the
// page never locks up, regardless of dataset size.
function computeWithProgress(items, processFn, onProgress, chunkSize){
  chunkSize = chunkSize || 400;
  return new Promise(resolve => {
    let i = 0;
    const results = [];
    function step(){
      const end = Math.min(i + chunkSize, items.length);
      for(; i < end; i++){
        results.push(processFn(items[i], i));
      }
      onProgress(items.length === 0 ? 100 : Math.round((i / items.length) * 100));
      if(i < items.length){
        requestAnimationFrame(step);
      } else {
        resolve(results);
      }
    }
    step();
  });
}

const ANALYTICS_META = {
  friction: { title: 'Friction ranking', desc: 'Which conversations or sessions had the highest share of flagged messages.' },
  trend: { title: 'Flag rate over time', desc: 'Percentage of messages flagged, tracked week by week or month by month.' },
  length: { title: 'Session length vs. flag rate', desc: 'Do longer sessions tend to have more flagged messages, proportionally?' },
  timeofday: { title: 'Time of day & day of week', desc: 'Is your flag rate higher at certain hours or on certain days?' },
  idlegap: { title: 'Idle time before a session', desc: 'Does picking a conversation back up after a long gap correlate with more friction?' },
};


function showAnalyticsProgress(){
  document.getElementById('analyticsMain').innerHTML = `
    <div class="progress-wrap">
      <div class="progress-track"><div class="progress-fill" id="analyticsProgressFill"></div></div>
      <div class="progress-label" id="analyticsProgressLabel">Computing…</div>
    </div>`;
}

function setAnalyticsProgress(pct){
  const fill = document.getElementById('analyticsProgressFill');
  const label = document.getElementById('analyticsProgressLabel');
  if(fill) fill.style.width = pct + '%';
  if(label) label.textContent = `Computing… ${pct}%`;
}

export function runAnalysis(name, opts){
  opts = opts || {};
  state.selectedAnalysis = name;
  rememberLocation();
  document.querySelectorAll('.analytics-item').forEach(b=>{
    b.classList.toggle('active', b.dataset.analysis === name);
  });
  showAnalyticsProgress();

  const [compute, render] = {
    friction: [computeFrictionAnalysis, renderFrictionResult],
    trend: [computeTrendAnalysis, renderTrendResult],
    length: [computeLengthAnalysis, renderLengthResult],
    timeofday: [computeTimeOfDayAnalysis, renderTimeOfDayResult],
    idlegap: [computeIdleGapAnalysis, renderIdleGapResult],
  }[name];

  // The computation runs in chunks, yielding to the browser between them so
  // the progress bar moves; drawing happens once it finishes.
  const runChunked = (items, fn) => computeWithProgress(items, fn, setAnalyticsProgress);
  compute(opts, runChunked).then(render);
}

function renderFrictionResult({ rows, granularity }){
  const meta = ANALYTICS_META.friction;
  const rowsHtml = rows.slice(0, 100).map(r => `
    <tr class="friction-row" data-conv="${r.conv}" data-start="${r.rangeStart||''}" data-end="${r.rangeEnd||''}">
      <td>${escapeHtml(r.label)}</td>
      <td>${r.total}</td>
      <td>${r.flagged}</td>
      <td class="pct">${r.pct.toFixed(1)}%</td>
    </tr>`).join('');

  document.getElementById('analyticsMain').innerHTML = `
    <h3>${meta.title}</h3>
    <p class="analytics-desc">${meta.desc} Showing top ${Math.min(100, rows.length)} of ${rows.length}. Click a row to open it in Review.</p>
    <div class="analytics-controls">
      <button class="seg ${granularity==='conversation'?'active':''}" id="frictionByConv">By conversation</button>
      <button class="seg ${granularity==='session'?'active':''}" id="frictionBySession">By session</button>
    </div>
    <table class="friction">
      <thead><tr><th>${granularity==='conversation'?'Conversation':'Session'}</th><th>Messages</th><th>Flagged</th><th>% flagged</th></tr></thead>
      <tbody>${rowsHtml}</tbody>
    </table>`;

  document.getElementById('frictionByConv').addEventListener('click', ()=> runAnalysis('friction', {granularity:'conversation'}));
  document.getElementById('frictionBySession').addEventListener('click', ()=> runAnalysis('friction', {granularity:'session'}));

  document.querySelectorAll('.friction-row').forEach(row=>{
    row.addEventListener('click', ()=>{
      const conv = parseInt(row.dataset.conv, 10);
      if(row.dataset.start){
        jumpToReview({ conv, rangeStart: parseInt(row.dataset.start,10), rangeEnd: parseInt(row.dataset.end,10), flagType: 'all' });
      } else {
        jumpToReview({ conv, flagType: 'all' });
      }
    });
  });
}

function renderTrendResult({ points, granularity }){
  const meta = ANALYTICS_META.trend;
  document.getElementById('analyticsMain').innerHTML = `
    <h3>${meta.title}</h3>
    <p class="analytics-desc">${meta.desc}</p>
    <div class="analytics-controls">
      <button class="seg ${granularity==='week'?'active':''}" id="trendWeekly">Weekly</button>
      <button class="seg ${granularity==='month'?'active':''}" id="trendMonthly">Monthly</button>
    </div>
    <div id="trendChart"></div>`;
  document.getElementById('trendWeekly').addEventListener('click', ()=> runAnalysis('trend', {granularity:'week'}));
  document.getElementById('trendMonthly').addEventListener('click', ()=> runAnalysis('trend', {granularity:'month'}));
  renderLineChartSVG(document.getElementById('trendChart'), points, {
    yLabel: '% flagged',
    tooltipFn: p => `${p.x}: ${p.y.toFixed(1)}% (${p.flagged}/${p.total})`,
  });
}

function renderLengthResult({ points }){
  const meta = ANALYTICS_META.length;
  const r = pearsonR(points.map(p=>p.x), points.map(p=>p.y));
  document.getElementById('analyticsMain').innerHTML = `
    <h3>${meta.title}</h3>
    <p class="analytics-desc">${meta.desc} Each dot is one session. Click a dot to open it in Review.</p>
    <div class="stat-row">
      <div class="stat-block"><span class="num">${r===null?'—':r.toFixed(2)}</span><span class="label">correlation (r)</span></div>
      <div class="stat-block"><span class="num">${points.length}</span><span class="label">sessions</span></div>
    </div>
    <div id="lengthChart"></div>`;
  renderScatterChartSVG(document.getElementById('lengthChart'), points, {
    xLabel: 'Session length (minutes)',
    yLabel: '% of session flagged',
    onPointClick: p => jumpToReview({ conv: p.conv, rangeStart: p.rangeStart, rangeEnd: p.rangeEnd, flagType: 'all' }),
  });
}

function renderTimeOfDayResult({ byHour, byDow }){
  const meta = ANALYTICS_META.timeofday;
  const dowNames = ['Sun','Mon','Tue','Wed','Thu','Fri','Sat'];
  document.getElementById('analyticsMain').innerHTML = `
    <h3>${meta.title}</h3>
    <p class="analytics-desc">${meta.desc} Bar height is % of messages flagged in that bucket; hover a bar for counts.</p>
    <p class="hint" style="margin-bottom:6px;">By hour of day (your local time)</p>
    <div id="hourChart" style="margin-bottom:28px;"></div>
    <p class="hint" style="margin-bottom:6px;">By day of week</p>
    <div id="dowChart"></div>`;

  renderBarChartSVG(document.getElementById('hourChart'),
    byHour.map((b,h)=>({ label: h % 3 === 0 ? h+':00' : '', value: b.total ? b.flagged/b.total*100 : 0, tooltip: `${h}:00 — ${b.total ? (b.flagged/b.total*100).toFixed(1) : 0}% (${b.flagged}/${b.total})` })),
    { yLabel: '% flagged' });

  renderBarChartSVG(document.getElementById('dowChart'),
    byDow.map((b,i)=>({ label: dowNames[i], value: b.total ? b.flagged/b.total*100 : 0, tooltip: `${dowNames[i]} — ${b.total ? (b.flagged/b.total*100).toFixed(1) : 0}% (${b.flagged}/${b.total})` })),
    { yLabel: '% flagged' });
}

function renderIdleGapResult({ points, excludedCount }){
  const meta = ANALYTICS_META.idlegap;
  const logXs = points.map(p => Math.log10(p.x));
  const r = pearsonR(logXs, points.map(p=>p.y));
  document.getElementById('analyticsMain').innerHTML = `
    <h3>${meta.title}</h3>
    <p class="analytics-desc">${meta.desc} X-axis is log-scaled (a session right after the last one looks very different from one picked up a week later). ${excludedCount} session${excludedCount===1?'':'s'} excluded as a conversation's first session (no prior gap to measure). Click a dot to open it in Review.</p>
    <div class="stat-row">
      <div class="stat-block"><span class="num">${r===null?'—':r.toFixed(2)}</span><span class="label">correlation (r, log-gap)</span></div>
      <div class="stat-block"><span class="num">${points.length}</span><span class="label">sessions with a prior gap</span></div>
    </div>
    <div id="idleGapChart"></div>`;
  renderScatterChartSVG(document.getElementById('idleGapChart'), points, {
    xLabel: 'Hours since previous session ended (log scale)',
    yLabel: '% of session flagged',
    xScale: 'log',
    onPointClick: p => jumpToReview({ conv: p.conv, rangeStart: p.rangeStart, rangeEnd: p.rangeEnd, flagType: 'all' }),
  });
}
