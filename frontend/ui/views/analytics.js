// Draws the Analytics tab (plan
// docs/plans/2026-10-06-load-only-what-the-page-shows.md §5c, §8c). Three
// analyses (friction ranking, session length, idle time) are computed here
// from the sessions' stored counts, in 50 ms turns so the bar moves and the
// page stays responsive (core/turns.js); two (flag rate over time, time of
// day) need each message's own time, so the server computes them, and the
// page asks again while it answers "working". Every analysis can stop
// part-way and carry on: leaving Analytics stops it, and coming back to the
// same analysis with the same options carries on from where it stopped (the
// page keeps its partial result; the server keeps its own). Results link
// through to the review tab.

import { PAGE_ANALYSES, createAnalysisRuns, pearsonR } from '../../core/analyses.js';
import { viewName } from '../../core/session-counts.js';
import { state } from '../../core/state.js';
import { runInTurns } from '../../core/turns.js';
import { TURN_MS, clockBudget } from '../../core/work-budget.js';
import { ensureAuthToken, fetchAnalysis } from '../../infra/api-client.js';
import { rememberLocation } from '../navigation/location.js';
import { renderBarChartSVG, renderLineChartSVG, renderScatterChartSVG } from '../render/charts.js';
import { escapeHtml } from '../render/markup.js';
import { createProgressBar } from '../widgets/status-indicators.js';
import { jumpToReview } from './review.js';

const ANALYTICS_META = {
  friction: { title: 'Friction ranking', desc: 'Which conversations or sessions had the highest share of flagged messages.' },
  trend: { title: 'Flag rate over time', desc: 'Percentage of messages flagged, tracked week by week or month by month.' },
  length: { title: 'Session length vs. flag rate', desc: 'Do longer sessions tend to have more flagged messages, proportionally?' },
  timeofday: { title: 'Time of day & day of week', desc: 'Is your flag rate higher at certain hours or on certain days?' },
  idlegap: { title: 'Idle time before a session', desc: 'Does picking a conversation back up after a long gap correlate with more friction?' },
};

// The two the server computes, by the names of their routes.
const SERVER_ROUTES = { trend: 'trend', timeofday: 'time-of-day' };

function showAnalyticsProgress(){
  document.getElementById('analyticsMain').innerHTML = `
    <div class="progress-wrap">
      <div class="progress-track"><div class="progress-fill" id="analyticsProgressFill"></div></div>
      <div class="progress-label" id="analyticsProgressLabel"></div>
    </div>`;
  return createProgressBar({
    nodes: () => ({ fill: document.getElementById('analyticsProgressFill'), label: document.getElementById('analyticsProgressLabel') }),
  });
}

// The options the shown analysis last ran with (e.g. by session or by
// month), so rerunAnalysis can redraw it the same way.
let lastOpts = {};
// Counts runs, so a slower earlier run can't draw over a later one, and
// stops asking once a later one starts.
let runCount = 0;
// The analyses computed here, finished or stopped part-way.
const RUNS = createAnalysisRuns();

// Recomputes the chosen analysis, if there is one, after the flags or the
// show switches change.
export function rerunAnalysis(){
  if(state.selectedAnalysis) runAnalysis(state.selectedAnalysis, lastOpts);
}

export function runAnalysis(name, opts){
  opts = opts || {};
  lastOpts = opts;
  const thisRun = ++runCount;
  state.selectedAnalysis = name;
  rememberLocation();
  document.querySelectorAll('.analytics-item').forEach(b=>{
    b.classList.toggle('active', b.dataset.analysis === name);
  });
  const bar = showAnalyticsProgress();
  // Still wanted: no later run, the same analysis, and Analytics on screen.
  const wanted = () => thisRun === runCount && state.selectedAnalysis === name
    && document.getElementById('view-analytics').classList.contains('active');
  const render = {
    friction: renderFrictionResult,
    trend: renderTrendResult,
    length: renderLengthResult,
    timeofday: renderTimeOfDayResult,
    idlegap: renderIdleGapResult,
  }[name];
  const run = PAGE_ANALYSES.includes(name) ? runInPage : runOnServer;
  run(name, opts, bar, wanted).then((result) => {
    bar.stop();
    if(result && wanted()) render(result);
  }, (err) => {
    console.error(err);
    bar.failed('progress.request_failed', { detail: err.message });
  });
}

// One of the three analyses computed here: carried on from where it
// stopped, if it was started before with the same options, view and data.
// Resolves to its result, or null when stopped.
async function runInPage(name, opts, bar, wanted){
  const data = { conversations: state.conversations, blocks: state.blocks, view: viewName(state.showAuto, state.showUser) };
  const run = RUNS.get(name, opts, data, state.dataVersion);
  if(!run.finished){
    bar.measured('progress.computing', {}, 0, data.blocks.length);
    const out = await runInTurns(run.steps, {
      budget: () => clockBudget(TURN_MS),
      nextTurn: () => new Promise((resolve) => setTimeout(resolve, 0)),
      onProgress: (p) => bar.measured('progress.computing', {}, p.done, p.total),
      stopped: () => !wanted(),
    });
    if(!out.finished) return null;
    run.finished = true;
    run.result = out.result;
  }
  return run.result;
}

// One of the two the server computes, asked again while it answers that it
// is still working; asking stops when Analytics is left. Resolves to its
// result, or null when stopped.
async function runOnServer(name, opts, bar, wanted){
  const view = viewName(state.showAuto, state.showUser);
  const params = { view, tz: Intl.DateTimeFormat().resolvedOptions().timeZone };
  if(name === 'trend') params.granularity = opts.granularity || 'week';
  bar.working('progress.server_computing', { done: 0, total: 0 });
  const token = await ensureAuthToken(document.getElementById('devLoginSub').value.trim());
  for(;;){
    if(!wanted()) return null;
    const answer = await fetchAnalysis(token, SERVER_ROUTES[name], params);
    if(answer.status === 'done') return name === 'trend' ? trendResult(answer.numbers) : timeOfDayResult(answer.numbers);
    bar.measured('progress.server_computing', {}, answer.sessions_done, answer.sessions_total);
  }
}

// The server's buckets as the chart's points, in time order. A bucket with
// no counted messages has no rate and is left out.
function trendResult(numbers){
  const points = Object.keys(numbers.buckets).sort()
    .filter((key) => numbers.buckets[key].total > 0)
    .map((key) => {
      const { total, flagged } = numbers.buckets[key];
      return { x: key, y: flagged / total * 100, total, flagged };
    });
  return { points, granularity: numbers.granularity };
}

function timeOfDayResult(numbers){
  return { byHour: numbers.by_hour, byDow: numbers.by_dow };
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
    xTooltip: x => `${x.toFixed(0)} min long`,
    onPointClick: p => jumpToReview({ conv: p.conv, rangeStart: p.rangeStart, rangeEnd: p.rangeEnd, flagType: 'all' }),
  });
}

// One bar of the time-of-day charts. A bucket with no counted messages has
// no rate, so it gets no bar rather than a misleading 0%.
function bucketBar(label, name, b){
  if(!b.total) return { label, value: null };
  const pct = b.flagged / b.total * 100;
  return { label, value: pct, tooltip: `${name} — ${pct.toFixed(1)}% (${b.flagged}/${b.total})` };
}

function renderTimeOfDayResult({ byHour, byDow }){
  const meta = ANALYTICS_META.timeofday;
  const dowNames = ['Sun','Mon','Tue','Wed','Thu','Fri','Sat'];
  document.getElementById('analyticsMain').innerHTML = `
    <h3>${meta.title}</h3>
    <p class="analytics-desc">${meta.desc} Bar height is % of messages flagged in that bucket; hover a bar for counts.</p>
    <p class="hint before-chart">By hour of day (your local time)</p>
    <div id="hourChart" class="chart-block"></div>
    <p class="hint before-chart">By day of week</p>
    <div id="dowChart"></div>`;

  renderBarChartSVG(document.getElementById('hourChart'),
    byHour.map((b,h)=>bucketBar(h % 3 === 0 ? h+':00' : '', `${h}:00`, b)),
    { yLabel: '% flagged' });

  renderBarChartSVG(document.getElementById('dowChart'),
    byDow.map((b,i)=>bucketBar(dowNames[i], dowNames[i], b)),
    { yLabel: '% flagged' });
}

function renderIdleGapResult({ points, excludedCount, uncountedCount }){
  const meta = ANALYTICS_META.idlegap;
  const logXs = points.map(p => Math.log10(p.x));
  const r = pearsonR(logXs, points.map(p=>p.y));
  document.getElementById('analyticsMain').innerHTML = `
    <h3>${meta.title}</h3>
    <p class="analytics-desc">${meta.desc} X-axis is log-scaled (a session right after the last one looks very different from one picked up a week later). ${excludedCount} session${excludedCount===1?'':'s'} excluded as a conversation's first session (no prior gap to measure)${uncountedCount ? `, and ${uncountedCount} with no reviewed messages` : ''}. Click a dot to open it in Review.</p>
    <div class="stat-row">
      <div class="stat-block"><span class="num">${r===null?'—':r.toFixed(2)}</span><span class="label">correlation (r, log-gap)</span></div>
      <div class="stat-block"><span class="num">${points.length}</span><span class="label">sessions with a prior gap</span></div>
    </div>
    <div id="idleGapChart"></div>`;
  renderScatterChartSVG(document.getElementById('idleGapChart'), points, {
    xLabel: 'Hours since previous session ended (log scale)',
    yLabel: '% of session flagged',
    xScale: 'log',
    xTooltip: x => `${x.toFixed(1)} h after the previous session`,
    onPointClick: p => jumpToReview({ conv: p.conv, rangeStart: p.rangeStart, rangeEnd: p.rangeEnd, flagType: 'all' }),
  });
}
