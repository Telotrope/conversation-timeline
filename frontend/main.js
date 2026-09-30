import { computeFrictionAnalysis, computeIdleGapAnalysis, computeLengthAnalysis, computeTimeOfDayAnalysis, computeTrendAnalysis, pearsonR } from './core/analyses.js';
import { buildBlocks, localDateKey } from './core/blocks.js';
import { FORMAT_VERSION, extractMessageText, parseUploadedConversations } from './core/export-format.js';
import { attachFlags, effectiveFlag, hasUserValue, isOverridden } from './core/flags.js';
import { fmtClock, fmtDayHeading, fmtDuration, fmtMonthHeading, formatBytes, formatEta } from './core/format.js';
import { state } from './core/state.js';
import { API_BASE, clearAuthToken, describeFailure, ensureAuthToken, patchFlagsToBackend, putWithProgress, readBodyWithProgress } from './infra/api-client.js';

function setLoadStatus(msg, isError){
  const el = document.getElementById('loadStatus');
  el.textContent = msg;
  el.style.color = isError ? '#B0392F' : 'var(--ink-faint)';
}

// --- Load-screen progress ---
// Three states, because the load has three genuinely different kinds of
// phase: a measurable transfer, an unmeasurable wait, and finished. Nothing
// here invents a percentage for work whose size isn't known.

function showLoadProgress(){
  const wrap = document.getElementById('loadProgress');
  const fill = document.getElementById('loadProgressFill');
  wrap.style.display = 'block';
  fill.classList.remove('is-error');
  fill.style.width = '0%';
  document.getElementById('loadProgressLabel').textContent = '';
}

function hideLoadProgress(){
  document.getElementById('loadProgress').style.display = 'none';
}

function failLoadProgress(){
  const wrap = document.getElementById('loadProgress');
  if(wrap.style.display === 'none') return;
  document.getElementById('loadProgressFill').classList.add('is-error');
}

// A phase whose duration can't be observed: full-width track, no number.
// Used for the stretch after the request body is fully sent but before the
// server answers -- the local-dev PUT handler does its processing there, and
// a bar frozen at 100% would read as hung.
function setLoadProgressIndeterminate(label){
  document.getElementById('loadProgress').style.display = 'block';
  document.getElementById('loadProgressFill').style.width = '100%';
  document.getElementById('loadProgressLabel').textContent = label;
}

// Estimates remaining time from a rolling window of recent progress events
// rather than an average over the whole transfer -- a whole-transfer average
// keeps reporting a stale rate long after the speed changes. Returns '' until
// there are at least two samples spanning enough time to mean anything.
function makeRateEstimator(windowMs){
  const samples = [];
  return function(loaded, total){
    const now = Date.now();
    samples.push({ t: now, loaded });
    while(samples.length > 2 && now - samples[0].t > windowMs) samples.shift();
    if(samples.length < 2) return '';
    const first = samples[0];
    const elapsed = (now - first.t) / 1000;
    const moved = loaded - first.loaded;
    if(elapsed < 1 || moved <= 0) return '';
    return formatEta((total - loaded) / (moved / elapsed));
  };
}

// Applies an already-downloaded export: parses it, replaces the page's
// state, and shows the timeline. Returns false if the export held no
// conversations, leaving the caller to report that however suits it.
//
// Shared by the upload path and the restore-on-load path below, so a
// restored session goes through exactly the same rendering as a fresh
// upload rather than a parallel copy that can drift.
function applyExportText(text){
  const parsed = parseUploadedConversations(text);
  if(parsed.conversations.length === 0) return false;

  state.conversations = parsed.conversations;
  state.messages = parsed.messages;
  state.humanMessages = parsed.humanMessages;
  state.humanById = new Map(state.humanMessages.map(m => [m.id, m]));
  state.blocks = buildBlocks();
  state.rawData = parsed.rawData;

  // Your confirmed flags come from whatever the server's export embedded
  // (overrides you PATCHed to the backend earlier -- see
  // patchFlagsToBackend). There's no other recovery mechanism; see the
  // migration plan's V2a.
  state.overrides = { ...parsed.embeddedOverrides };
  const embeddedCount = Object.keys(parsed.embeddedOverrides).length;

  attachFlags();
  if(embeddedCount > 0){
    setSaveStatus(`Loaded ${embeddedCount} of your confirmed flag${embeddedCount===1?'':'s'} from the server.`);
  }

  document.getElementById('loadScreen').style.display = 'none';
  document.getElementById('mainContent').style.display = '';

  renderSubtitle();
  renderCalendar();
  renderConvList();
  renderReviewTable();
  return true;
}

// Picks up the last session on load, so a reload or a Back press past the
// first entry doesn't cost you the whole upload again. The backend still
// holds the processed export; all this needs is the name it was uploaded
// under.
//
// Announced rather than silent: a page that quietly opens with old data
// leaves you unsure whether you're looking at this file or the last one.
// Everything about it is best-effort -- no remembered name, a server that
// was restarted (its storage is in-memory), or anything else unexpected
// just means the normal load screen, which is the correct fallback and not
// an error worth shouting about.
async function tryRestoreSession(){
  let sub = null;
  try{ sub = localStorage.getItem('timeline_dev_sub'); } catch(e){ return; }
  if(!sub) return;

  document.getElementById('devLoginSub').value = sub;
  try{
    const token = await ensureAuthToken(document.getElementById('devLoginSub').value.trim());
    const exportRes = await fetch(`${API_BASE}/export`, {
      headers: { 'Authorization': `Bearer ${token}` },
    });
    if(!exportRes.ok) return;
    const { export_url } = await exportRes.json();
    const downloadRes = await fetch(`${API_BASE}${export_url}`);
    if(!downloadRes.ok) return;
    if(!applyExportText(await downloadRes.text())) return;

    showRestoredNotice(sub);
    applyLocationHash();
  } catch(e){
    // The server being gone or unreachable is the ordinary case here, not a
    // fault: it just means there is nothing to restore. Logged rather than
    // swallowed so a genuinely surprising failure is still visible.
    console.info('No previous session restored:', e.message);
    clearAuthToken();
  }
}

function showRestoredNotice(sub){
  const notice = document.getElementById('restoredNotice');
  document.getElementById('restoredNoticeText').textContent =
    `Picked up where you left off — the export you last loaded as "${sub}". Use "Load a different file…" to start fresh.`;
  notice.style.display = 'flex';
  document.getElementById('restoredNoticeDismiss').onclick = () => { notice.style.display = 'none'; };
}

// Runs the backend's non-generative detection pass, one page of
// conversations at a time. The server could do the whole thing in one
// request, but then there would be nothing to report: paging is what makes
// the progress bar show real, earned progress rather than a spinner.
async function runDetectionPass(token){
  const fill = document.getElementById('loadProgressFill');
  const label = document.getElementById('loadProgressLabel');
  let offset = 0;
  let detected = 0;
  for(;;){
    const res = await fetch(`${API_BASE}/detect`, {
      method: 'POST',
      headers: { 'Authorization': `Bearer ${token}`, 'Content-Type': 'application/json' },
      body: JSON.stringify({ offset, limit: 5 }),
    });
    if(!res.ok) throw new Error(await describeFailure('scanning your messages', res));
    const body = await res.json();
    detected += body.messages_detected;

    const total = body.total_conversations;
    const done = body.next_offset === null || body.next_offset === undefined;
    const covered = done ? total : body.next_offset;
    const pct = total ? Math.round((covered / total) * 100) : 100;
    fill.style.width = pct + '%';
    label.textContent =
      `Scanning your messages — ${covered} of ${total} conversation${total === 1 ? '' : 's'} (${pct}%)`;

    if(done) return detected;
    offset = body.next_offset;
  }
}

async function handleLoadClick(){
  const convInput = document.getElementById('loadConvFile');
  const convFile = convInput.files[0];
  const runDetection = document.getElementById('autoDetectCheckbox').checked;

  if(!convFile){
    setLoadStatus('Choose a conversations.json file first.', true);
    hideLoadProgress();
    return;
  }

  try{
    showLoadProgress();
    setLoadStatus('Logging in…');
    setLoadProgressIndeterminate('Signing in…');
    const token = await ensureAuthToken(document.getElementById('devLoginSub').value.trim());

    setLoadStatus('Uploading your conversation export…');
    setLoadProgressIndeterminate('Reading the file…');
    const rawText = await convFile.text();
    const createRes = await fetch(`${API_BASE}/uploads`, {
      method: 'POST',
      headers: { 'Authorization': `Bearer ${token}` },
    });
    if(!createRes.ok) throw new Error(await describeFailure('starting the upload', createRes));
    const { upload_url } = await createRes.json();

    const uploadFill = document.getElementById('loadProgressFill');
    const uploadLabel = document.getElementById('loadProgressLabel');
    const uploadEta = makeRateEstimator(3000);
    const putRes = await putWithProgress(`${API_BASE}${upload_url}`, rawText, (loaded, total) => {
      const pct = Math.round((loaded / total) * 100);
      uploadFill.style.width = pct + '%';
      const eta = uploadEta(loaded, total);
      uploadLabel.textContent =
        `Uploading ${formatBytes(loaded)} of ${formatBytes(total)} (${pct}%)` + (eta ? ` — ${eta}` : '');
    });
    if(!putRes.ok){
      throw new Error(`uploading the file failed (${putRes.status})${putRes.text ? ': ' + putRes.text : ''}`);
    }

    // The bytes being sent is not the end of the wait: the local-dev PUT
    // handler parses, dedups and stores the upload before it answers, and
    // none of that is observable from here. Saying so beats a full bar that
    // looks stuck. No polling is needed either -- by the time the PUT
    // resolves the work is done. Real S3-triggered processing is
    // asynchronous; that gap isn't solved here, see the migration plan's V2a.
    setLoadStatus('Processing your export…');
    setLoadProgressIndeterminate('Finishing up on the server…');

    // Only if asked. Detection reads every message you sent, and nothing
    // here has measured how long that takes, so it is never implied by the
    // act of uploading.
    if(runDetection){
      setLoadStatus('Scanning your messages for flags…');
      await runDetectionPass(token);
      setLoadProgressIndeterminate('Finishing up on the server…');
    }

    const exportRes = await fetch(`${API_BASE}/export`, {
      headers: { 'Authorization': `Bearer ${token}` },
    });
    if(!exportRes.ok) throw new Error(await describeFailure('reading back the processed export', exportRes));
    const { export_url } = await exportRes.json();

    const downloadRes = await fetch(`${API_BASE}${export_url}`);
    if(!downloadRes.ok) throw new Error(await describeFailure('downloading the processed export', downloadRes));
    const downloadEta = makeRateEstimator(3000);
    const text = await readBodyWithProgress(downloadRes, (loaded, total) => {
      if(total){
        const pct = Math.round((loaded / total) * 100);
        uploadFill.style.width = pct + '%';
        const eta = downloadEta(loaded, total);
        uploadLabel.textContent =
          `Downloading ${formatBytes(loaded)} of ${formatBytes(total)} (${pct}%)` + (eta ? ` — ${eta}` : '');
      } else {
        // No Content-Length: report what has actually arrived rather than
        // inventing a proportion of an unknown whole.
        uploadFill.style.width = '100%';
        uploadLabel.textContent = `Downloading… ${formatBytes(loaded)} so far`;
      }
    });

    setLoadProgressIndeterminate('Preparing the timeline…');

    if(!applyExportText(text)){
      setLoadStatus('That file parsed, but contained no conversations — is it the right export?', true);
      failLoadProgress();
      return;
    }
    hideLoadProgress();
  } catch(err){
    console.error(err);
    failLoadProgress();
    // A TypeError here (not an HTTP error response -- those are handled by
    // describeFailure above) means fetch() itself couldn't reach the
    // server at all -- almost always because it isn't running.
    const hint = err instanceof TypeError
      ? ` Is the backend running (cargo run -p timeline-api) at ${API_BASE}?`
      : '';
    setLoadStatus('Could not load that file through the backend — ' + err.message + hint, true);
  }
}

document.getElementById('loadBtn').addEventListener('click', handleLoadClick);
document.getElementById('loadDifferentBtn').addEventListener('click', ()=>{
  document.getElementById('mainContent').style.display = 'none';
  document.getElementById('loadScreen').style.display = '';
  document.getElementById('loadConvFile').value = '';
  setLoadStatus('');
  // Asking for a different file is also how you say "stop bringing the old
  // one back", so the remembered session goes with it. Without this, the
  // next reload would silently restore exactly what you just dismissed.
  try{ localStorage.removeItem('timeline_dev_sub'); } catch(e){ /* nothing to forget */ }
  clearAuthToken();
  window.location.hash = '';
});

state.blocks = buildBlocks();
attachFlags();

function setSaveStatus(msg){
  const el = document.getElementById('saveStatus');
  if(el) el.textContent = msg;
}

// Clicking any single checkbox, or the Approve button, promotes ALL THREE
// flags on that message to explicit user values at once — using the
// just-changed value for `changedType` (if any) and the message's current
// effective value for the other two. This is what "approving a row" means:
// one click reviews the whole message, not just the box you touched.
function setRowOverrides(id, changedType, changedValue){
  const msg = state.humanById.get(id);
  if(!msg) return;
  const values = {
    caps: changedType === 'caps' ? changedValue : effectiveFlag(msg, 'caps'),
    angry: changedType === 'angry' ? changedValue : effectiveFlag(msg, 'angry'),
    critical: changedType === 'critical' ? changedValue : effectiveFlag(msg, 'critical'),
  };
  state.overrides[id] = values;
  attachFlags();
  patchFlagsToBackend(msg, values).then(setSaveStatus);
  renderCalendar();
  renderConvList(document.getElementById('convSearch').value);
  if(state.selectedConversation !== null) selectConversation(state.selectedConversation);
  renderReviewTable();
}

function approveRow(id){
  setRowOverrides(id, null, null);
}

// Writes both the auto-detected values and your confirmed overrides onto
// each message (in two separate, namespaced fields, so a future load can
// never confuse one for the other), wraps the whole thing with a format
// version marker, and downloads it. This is now the only save mechanism —
// one self-contained file carries the conversation data, the automatic
// tags, and your corrections together.
function exportAnnotatedConversations(){
  if(!state.rawData){
    setSaveStatus('No conversation data loaded to annotate.');
    return;
  }
  // Mutate state.rawData directly rather than deep-cloning it first — for a
  // file this size, a stringify-then-reparse clone briefly needs 2-3x the
  // data's size in memory all at once (original + serialized string +
  // freshly parsed copy), which is enough to crash the tab outright on a
  // large export. There's nothing unsafe about mutating in place here:
  // we only ever add two clearly namespaced fields to human messages,
  // never remove or alter anything else, so doing it again on a later
  // export is harmless and idempotent.
  const annotated = state.rawData;
  annotated.forEach((c, convIdx) => {
    (c.chat_messages || []).forEach(m => {
      if(m.sender !== 'human') return;
      const id = convIdx + '|' + m.created_at;
      delete m._claude_timeline_flags; // retire the old single-field format

      const msg = state.humanById.get(id);
      if(msg){
        m._claude_timeline_auto = {
          caps: msg.default_caps,
          angry: msg.default_angry,
          critical: msg.default_critical,
          source: msg.auto_source || 'heuristic',
        };
      }

      if(state.overrides[id] && Object.keys(state.overrides[id]).length){
        m._claude_timeline_user = state.overrides[id];
      } else {
        delete m._claude_timeline_user;
      }
    });
  });

  const wrapped = { claude_timeline_format_version: FORMAT_VERSION, conversations: annotated };

  const blob = new Blob([JSON.stringify(wrapped)], {type:'application/json'});
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = 'conversations-with-flags.json';
  a.click();
  URL.revokeObjectURL(url);
  setSaveStatus('Downloaded conversations-with-flags.json — load this file directly next time.');
}

const PALETTE = ['#3C6E64','#A6752C','#7C5C8C','#4E7BA8','#B0553F','#5E8A4E','#8C6B4F','#3E6E8E','#9C5B6E','#6E7A3C'];
function colorFor(idx){ return PALETTE[idx % PALETTE.length]; }

// --- Header stats ---
function renderSubtitle(){
  const totalMsgs = state.conversations.reduce((a,c)=>a+c.total_messages,0);
  const dates = state.blocks.map(b=>b.date).sort();
  document.getElementById('subtitle').textContent =
    `${state.conversations.length} conversations, ${totalMsgs.toLocaleString()} messages, ${dates[0]} to ${dates[dates.length-1]}.`;
}

// --- Calendar view ---
function renderCalendar(){
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

// --- Conversation list & detail ---
function renderConvList(filter=''){
  const f = filter.trim().toLowerCase();
  const items = state.conversations
    .map((c, idx) => ({...c, idx}))
    .filter(c => c.name.toLowerCase().includes(f));

  document.getElementById('convItems').innerHTML = items.map(c => {
    const convBlocks = state.blocks.filter(b => b.conv === c.idx);
    const totalSec = convBlocks.reduce((a,b)=>a+b.duration_sec, 0);
    const dayCount = convBlocks.length;
    const hasCrit = convBlocks.some(b => b.criticalItems.length);
    const hasAngry = convBlocks.some(b => b.angryItems.length);
    const hasCaps = convBlocks.some(b => b.capsItems.length);
    let icons = '';
    if(hasCrit) icons += '<span class="flag-icon critical">⚑</span> ';
    if(hasAngry) icons += '<span class="flag-icon angry">!</span> ';
    if(hasCaps) icons += '<span class="flag-icon caps">A</span> ';
    return `<div class="conv-item" data-idx="${c.idx}">
      ${icons}${escapeHtml(c.name)}
      <span class="meta">${c.total_messages} messages · ${dayCount} ${dayCount===1?'day':'days'} · ${fmtDuration(totalSec)} active</span>
    </div>`;
  }).join('');

  document.querySelectorAll('.conv-item').forEach(el=>{
    el.addEventListener('click', ()=> selectConversation(parseInt(el.dataset.idx,10)));
  });
}

function selectConversation(idx){
  state.selectedConversation = idx;
  rememberLocation();
  document.querySelectorAll('.conv-item').forEach(el=>{
    el.classList.toggle('selected', parseInt(el.dataset.idx,10) === idx);
  });
  const conv = state.conversations[idx];
  const convBlocks = state.blocks.filter(b => b.conv === idx).sort((a,b)=> a.date.localeCompare(b.date));
  const totalSec = convBlocks.reduce((a,b)=>a+b.duration_sec, 0);

  const critCount = convBlocks.reduce((a,b)=> a + b.criticalItems.length, 0);
  const angryCount = convBlocks.reduce((a,b)=> a + b.angryItems.length, 0);
  const capsCount = convBlocks.reduce((a,b)=> a + b.capsItems.length, 0);

  const rows = convBlocks.map((b, bi) => {
    const flags = [];
    if(b.criticalItems.length) flags.push(`<span data-block-idx="${b._idx}" data-flag-type="critical" class="flag-icon critical" style="cursor:pointer;" title="${b.criticalItems.length} critical — click to review">⚑</span>`);
    if(b.angryItems.length) flags.push(`<span data-block-idx="${b._idx}" data-flag-type="angry" class="flag-icon angry" style="cursor:pointer;" title="${b.angryItems.length} angry — click to review">!</span>`);
    if(b.capsItems.length) flags.push(`<span data-block-idx="${b._idx}" data-flag-type="caps" class="flag-icon caps" style="cursor:pointer;" title="${b.capsItems.length} ALL-CAPS — click to review">A</span>`);
    return `
    <tr class="session-row" data-block-idx="${b._idx}" style="cursor:pointer;" title="Click to review these messages">
      <td>${fmtDayHeading(b.date)}</td>
      <td>${fmtClock(b.start)} – ${fmtClock(b.end)}</td>
      <td class="dur">${fmtDuration(b.duration_sec)}</td>
      <td>${b.count}</td>
      <td>${flags.join(' ')}</td>
    </tr>`;
  }).join('');

  const notes = [];
  if(critCount) notes.push(`<span class="flag-icon critical">⚑</span> ${critCount} critical`);
  if(angryCount) notes.push(`<span class="flag-icon angry">!</span> ${angryCount} angry`);
  if(capsCount) notes.push(`<span class="flag-icon caps">A</span> ${capsCount} ALL-CAPS`);
  const critNote = notes.length
    ? `<div class="summary" style="display:flex; gap:14px; align-items:center;">${notes.join('')} — click a flag or a row below to review those messages.</div>`
    : '';

  document.getElementById('convDetail').innerHTML = `
    <h3>${escapeHtml(conv.name)}</h3>
    <div class="summary">${conv.total_messages} messages total · active across ${convBlocks.length} ${convBlocks.length===1?'day':'days'} · ${fmtDuration(totalSec)} of combined active time</div>
    ${critNote}
    <table class="sessions">
      <thead><tr><th>Day</th><th>Time span</th><th>Duration</th><th>Messages</th><th>Flags</th></tr></thead>
      <tbody>${rows}</tbody>
    </table>
    <a href="#" id="chatReviewLink" style="display:inline-block; margin-top:18px; font-size:0.85rem; color:var(--accent);">Chat message review →</a>`;

  // Flag icon click: jump to Review showing the whole session for context,
  // with the flagged message(s) highlighted — not filtered to just that type.
  document.querySelectorAll('#convDetail .flag-icon[data-flag-type]').forEach(el=>{
    el.addEventListener('click', (e)=>{
      e.stopPropagation();
      const b = state.blocks[parseInt(el.dataset.blockIdx, 10)];
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

  // Row click (not on a flag icon): jump to Review showing all messages in that session
  document.querySelectorAll('#convDetail .session-row').forEach(el=>{
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

  // "Chat message review" link: jump to Review showing the whole conversation, unrestricted by time
  document.getElementById('chatReviewLink').addEventListener('click', (e)=>{
    e.preventDefault();
    jumpToReview({ conv: idx, flagType: 'all' });
  });
}

function escapeHtml(s){
  const div = document.createElement('div');
  div.textContent = s;
  return div.innerHTML;
}

// A small, dependency-free markdown renderer for displaying message text —
// handles what actually shows up in real conversations (headers, bold,
// italic, inline code, bullet/numbered lists, paragraph breaks) without
// pulling in a markdown library for a self-contained offline page. Escapes
// the raw text FIRST, then only ever adds tags on top of that escaped
// text — so nothing in the original message can inject real HTML.
function renderMarkdownLite(text){
  if(!text) return '';
  const escaped = escapeHtml(text);

  function inlineFormat(line){
    line = line.replace(/\*\*(.+?)\*\*/g, '<strong>$1</strong>');
    line = line.replace(/__(.+?)__/g, '<strong>$1</strong>');
    line = line.replace(/(^|[^*])\*([^*\n]+)\*(?!\*)/g, '$1<em>$2</em>');
    line = line.replace(/(^|[^_])_([^_\n]+)_(?!_)/g, '$1<em>$2</em>');
    line = line.replace(/`([^`]+)`/g, '<code style="background:var(--paper); padding:1px 4px; border-radius:3px; font-size:0.9em;">$1</code>');
    return line;
  }

  const lines = escaped.split('\n');
  let html = '';
  let inUl = false, inOl = false;
  let paragraphBuffer = [];

  function flushParagraph(){
    if(paragraphBuffer.length){
      html += '<p style="margin:0 0 8px;">' + paragraphBuffer.join('<br>') + '</p>';
      paragraphBuffer = [];
    }
  }
  function closeLists(){
    if(inUl){ html += '</ul>'; inUl = false; }
    if(inOl){ html += '</ol>'; inOl = false; }
  }

  lines.forEach(line => {
    const trimmed = line.trim();

    if(trimmed === ''){
      flushParagraph();
      closeLists();
      return;
    }

    const headerMatch = trimmed.match(/^(#{1,6})\s+(.*)$/);
    if(headerMatch){
      flushParagraph();
      closeLists();
      const size = Math.max(0.85, 1.15 - headerMatch[1].length * 0.08);
      html += `<div style="font-weight:600; font-size:${size}rem; margin:8px 0 4px;">${inlineFormat(headerMatch[2])}</div>`;
      return;
    }

    const ulMatch = trimmed.match(/^[-*]\s+(.*)$/);
    if(ulMatch){
      flushParagraph();
      if(inOl){ html += '</ol>'; inOl = false; }
      if(!inUl){ html += '<ul style="margin:2px 0 8px; padding-left:20px;">'; inUl = true; }
      html += `<li>${inlineFormat(ulMatch[1])}</li>`;
      return;
    }

    const olMatch = trimmed.match(/^\d+\.\s+(.*)$/);
    if(olMatch){
      flushParagraph();
      if(inUl){ html += '</ul>'; inUl = false; }
      if(!inOl){ html += '<ol style="margin:2px 0 8px; padding-left:20px;">'; inOl = true; }
      html += `<li>${inlineFormat(olMatch[1])}</li>`;
      return;
    }

    closeLists();
    paragraphBuffer.push(inlineFormat(line));
  });

  flushParagraph();
  closeLists();
  return html || escaped;
}

// --- Tabs ---
function switchTab(name){
  document.querySelectorAll('nav.tabs button').forEach(b=>{
    b.classList.toggle('active', b.dataset.tab === name);
  });
  document.querySelectorAll('.view').forEach(v=>{
    v.classList.toggle('active', v.id === 'view-' + name);
  });
}

// --- Where you are, in the URL ---
// Without this, every tab change and conversation selection is invisible to
// the browser, so Back leaves the page entirely and the whole export has to
// be loaded again. The three axes worth remembering are the tab, the open
// conversation, and the chosen analysis.
//
// A hash, not history.pushState: this file is still opened directly as a
// file:// URL in places (the e2e suite does exactly that), and pushState is
// restricted for file URLs in some browsers, so it would work when tested by
// hand and fail in the test meant to protect it. A hash behaves identically
// under both.
//
// Review search and pagination deliberately stay out -- they change on every
// keystroke and would bury real navigation under dozens of history entries.

// Set while applying a hash, so restoring state doesn't immediately write
// the same hash back and fight with the browser's own history.
let APPLYING_HASH = false;

function currentLocationHash(){
  if(state.selectedAnalysis && document.getElementById('view-analytics').classList.contains('active')){
    return `#analytics/${state.selectedAnalysis}`;
  }
  const active = document.querySelector('nav.tabs button.active');
  const tab = active ? active.dataset.tab : 'calendar';
  if(tab === 'conversations' && state.selectedConversation !== null) return `#conversations/${state.selectedConversation}`;
  return `#${tab}`;
}

function rememberLocation(){
  if(APPLYING_HASH) return;
  const next = currentLocationHash();
  if(next !== window.location.hash) window.location.hash = next;
}

function applyLocationHash(){
  const raw = window.location.hash.replace(/^#/, '');
  if(!raw) return;
  const [tab, arg] = raw.split('/');
  if(!document.getElementById('view-' + tab)) return;

  APPLYING_HASH = true;
  try{
    switchTab(tab);
    if(tab === 'conversations' && arg !== undefined){
      const idx = parseInt(arg, 10);
      if(!isNaN(idx) && idx >= 0 && idx < state.conversations.length) selectConversation(idx);
    }
    if(tab === 'analytics' && arg){
      const btn = document.querySelector(`.analytics-item[data-analysis="${arg}"]`);
      if(btn) runAnalysis(arg, {});
    }
  } finally {
    APPLYING_HASH = false;
  }
}

window.addEventListener('hashchange', applyLocationHash);
document.querySelectorAll('nav.tabs button').forEach(b=>{
  b.addEventListener('click', ()=>{ switchTab(b.dataset.tab); rememberLocation(); });
});

document.getElementById('convSearch').addEventListener('input', (e)=> renderConvList(e.target.value));

// --- Review tab ---
const PAGE_SIZE = 50;
let reviewPage = 0;
let reviewConvFilter = null;   // conversation index, or null for all conversations
let reviewRangeFilter = null;  // {start, end} in ms, or null for no time restriction
let reviewDayFilter = null;    // 'YYYY-MM-DD' (local), or null — mutually exclusive with conv/range
let reviewHighlightIds = null; // message ids to flash/scroll to after render

function getFilteredHumanMessages(){
  const search = document.getElementById('reviewSearch').value.trim().toLowerCase();
  const filter = document.getElementById('reviewFilter').value;

  let results = state.humanMessages.filter(m=>{
    if(reviewDayFilter !== null){
      if(localDateKey(new Date(m.ts)) !== reviewDayFilter) return false;
    } else {
      if(reviewConvFilter !== null && m.conv !== reviewConvFilter) return false;
      if(reviewRangeFilter){
        const t = new Date(m.ts).getTime();
        if(t < reviewRangeFilter.start || t > reviewRangeFilter.end) return false;
      }
    }
    if(search && !m.text.toLowerCase().includes(search)) return false;
    const eCrit = effectiveFlag(m, 'critical');
    const eAngry = effectiveFlag(m, 'angry');
    const eCaps = effectiveFlag(m, 'caps');
    if(filter === 'flagged' && !(eCrit || eAngry || eCaps)) return false;
    if(filter === 'caps' && !eCaps) return false;
    if(filter === 'angry' && !eAngry) return false;
    if(filter === 'critical' && !eCrit) return false;
    if(filter === 'overridden' && !(isOverridden(m,'critical') || isOverridden(m,'angry') || isOverridden(m,'caps'))) return false;
    return true;
  });

  if(reviewDayFilter !== null){
    // Day view: grouped by conversation name, chronological within each —
    // never interleaved across conversations.
    results = results.slice().sort((a,b)=>{
      const nameA = state.conversations[a.conv].name, nameB = state.conversations[b.conv].name;
      if(nameA !== nameB) return nameA < nameB ? -1 : 1;
      return new Date(a.ts) - new Date(b.ts);
    });
  } else {
    results = results.slice().sort((a,b)=> new Date(a.ts) - new Date(b.ts));
  }
  return results;
}

// Jump into the Review tab from elsewhere on the page (a flag icon, a
// session time span, or a "chat message review" link), optionally
// restricted to one conversation and/or one time range, and optionally
// with specific rows flashed/scrolled into view once rendered.
function jumpToReview({conv=null, rangeStart=null, rangeEnd=null, flagType='all', highlightIds=null} = {}){
  reviewConvFilter = conv;
  reviewRangeFilter = (rangeStart != null && rangeEnd != null) ? {start: rangeStart, end: rangeEnd} : null;
  reviewDayFilter = null;
  reviewHighlightIds = highlightIds;
  document.getElementById('reviewSearch').value = '';
  document.getElementById('reviewFilter').value = flagType;
  reviewPage = 0;
  switchTab('review');
  renderReviewTable();
}

// Jump straight to a whole-day view across every conversation active that
// day (from clicking a date label on the Calendar tab, or "View entire day"
// from a narrower filter).
function jumpToReviewDay(dateKey){
  reviewDayFilter = dateKey;
  reviewConvFilter = null;
  reviewRangeFilter = null;
  reviewHighlightIds = null;
  document.getElementById('reviewSearch').value = '';
  document.getElementById('reviewFilter').value = 'all';
  reviewPage = 0;
  switchTab('review');
  renderReviewTable();
}

function shiftReviewDay(deltaDays){
  if(reviewDayFilter === null) return;
  const d = new Date(reviewDayFilter + 'T00:00:00');
  d.setDate(d.getDate() + deltaDays);
  reviewDayFilter = localDateKey(d);
  reviewPage = 0;
  renderReviewTable();
}

function clearReviewFilters(){
  reviewConvFilter = null;
  reviewRangeFilter = null;
  reviewDayFilter = null;
  reviewHighlightIds = null;
  document.getElementById('reviewFilter').value = 'all';
  reviewPage = 0;
  renderReviewTable();
}

// Renders a flag's checkbox cell according to the current state.showAuto/state.showUser
// state (see the four-row table in effectiveFlag's comment):
//  - both on:  editable, labeled "auto"/"you"
//  - auto only: read-only, shows auto value, no label
//  - your tags only: editable, labeled "tagged"/"untagged" (stated vs not)
//  - both off: caller skips this entirely (no columns at all)
function checkboxCell(msg, type){
  const val = effectiveFlag(msg, type);
  const autoTitle = msg.auto_source === 'llm' ? 'title="automatic tag from Claude, zero-shot"' : (msg.auto_source === 'heuristic' ? 'title="automatic tag from keyword/sentiment heuristic"' : '');
  if(state.showAuto && !state.showUser){
    return `<div class="flag-checkbox">
      <input type="checkbox" disabled ${val ? 'checked' : ''}>
    </div>`;
  }
  if(!state.showAuto && state.showUser){
    const stated = hasUserValue(msg, type);
    return `<div class="flag-checkbox">
      <input type="checkbox" data-id="${msg.id}" data-type="${type}" ${val ? 'checked' : ''}>
      <span class="src">${stated ? 'tagged' : 'untagged'}</span>
    </div>`;
  }
  // both on
  const overridden = isOverridden(msg, type);
  return `<div class="flag-checkbox${overridden ? ' is-override' : ''}">
    <input type="checkbox" data-id="${msg.id}" data-type="${type}" ${val ? 'checked' : ''}>
    <span class="src" ${overridden ? '' : autoTitle}>${overridden ? 'you' : (msg.auto_source === 'llm' ? 'AI' : 'auto')}</span>
  </div>`;
}

function renderReviewFilterBanner(){
  const el = document.getElementById('reviewFilterBanner');

  if(reviewDayFilter !== null){
    el.style.display = 'flex';
    const d = new Date(reviewDayFilter + 'T00:00:00');
    const label = d.toLocaleDateString(undefined, {weekday:'long', month:'long', day:'numeric', year:'numeric'});
    el.innerHTML = `
      <span>
        <button id="prevDayBtn" class="btn-secondary" style="padding:4px 10px;">◀</button>
        Day: <strong>${label}</strong>
        <button id="nextDayBtn" class="btn-secondary" style="padding:4px 10px;">▶</button>
      </span>
      <button id="clearReviewFilter" class="btn-secondary">Clear filter</button>`;
    document.getElementById('prevDayBtn').addEventListener('click', ()=> shiftReviewDay(-1));
    document.getElementById('nextDayBtn').addEventListener('click', ()=> shiftReviewDay(1));
    document.getElementById('clearReviewFilter').addEventListener('click', clearReviewFilters);
    return;
  }

  if(reviewConvFilter === null && !reviewRangeFilter){
    el.style.display = 'none';
    el.innerHTML = '';
    return;
  }
  el.style.display = 'flex';
  const convName = reviewConvFilter !== null ? state.conversations[reviewConvFilter].name : null;
  let label = '';
  if(convName) label += `Conversation: <strong>${escapeHtml(convName)}</strong>`;
  let dayKeyForButton = null;
  if(reviewRangeFilter){
    const s = new Date(reviewRangeFilter.start), e = new Date(reviewRangeFilter.end);
    dayKeyForButton = localDateKey(s);
    label += `${label ? ' · ' : ''}Time span: <strong>${s.toLocaleString(undefined,{month:'short',day:'numeric',hour:'numeric',minute:'2-digit'})} – ${e.toLocaleTimeString(undefined,{hour:'numeric',minute:'2-digit'})}</strong>`;
  }

  const buttons = [];
  if(reviewRangeFilter && reviewConvFilter !== null){
    buttons.push(`<button id="viewEntireConvBtn" class="btn-secondary">View entire conversation</button>`);
    buttons.push(`<button id="viewEntireDayBtn" class="btn-secondary">View entire day</button>`);
  }
  buttons.push(`<button id="clearReviewFilter" class="btn-secondary">Clear filter</button>`);

  el.innerHTML = `<span>${label}</span><span style="display:flex; gap:8px;">${buttons.join('')}</span>`;
  document.getElementById('clearReviewFilter').addEventListener('click', clearReviewFilters);
  const viewConvBtn = document.getElementById('viewEntireConvBtn');
  if(viewConvBtn) viewConvBtn.addEventListener('click', ()=>{
    reviewRangeFilter = null;
    reviewPage = 0;
    renderReviewTable();
  });
  const viewDayBtn = document.getElementById('viewEntireDayBtn');
  if(viewDayBtn) viewDayBtn.addEventListener('click', ()=> jumpToReviewDay(dayKeyForButton));
}

function renderReplyRow(msg){
  if(!state.showReplies) return '';
  const convMsgs = state.rawData[msg.conv] && state.rawData[msg.conv].chat_messages;
  if(!convMsgs) return '';
  const next = convMsgs[msg.rawIndex + 1];
  if(!next || next.sender !== 'assistant') return '';
  const replyText = extractMessageText(next);
  if(!replyText) return '';
  const colCount = (state.showAuto || state.showUser) ? 6 : 3;
  return `<tr class="claude-reply-row">
    <td colspan="${colCount}"><span class="who-label">Claude</span>${renderMarkdownLite(replyText)}</td>
  </tr>`;
}

function renderReviewTable(){
  renderReviewFilterBanner();
  const filtered = getFilteredHumanMessages();
  const totalPages = Math.max(1, Math.ceil(filtered.length / PAGE_SIZE));
  reviewPage = Math.min(reviewPage, totalPages - 1);
  const pageItems = filtered.slice(reviewPage * PAGE_SIZE, (reviewPage+1) * PAGE_SIZE);

  document.getElementById('reviewCount').textContent = `${filtered.length} message${filtered.length===1?'':'s'}`;

  const showFlagColumns = state.showAuto || state.showUser;

  const rows = pageItems.map(m => {
    const conv = state.conversations[m.conv];
    const dt = new Date(m.ts);
    const flagCells = showFlagColumns ? `
      <td class="flag-cell">${checkboxCell(m, 'caps')}</td>
      <td class="flag-cell">${checkboxCell(m, 'angry')}</td>
      <td class="flag-cell">${checkboxCell(m, 'critical')}</td>
      ${state.showUser ? `<td class="flag-cell"><button class="approve-btn" data-id="${m.id}">Approve</button></td>` : ''}
    ` : '';
    const mainRow = `<tr data-msg-id="${m.id}">
      <td class="when">${dt.toLocaleDateString(undefined,{month:'short',day:'numeric',year:'numeric'})}<br>${dt.toLocaleTimeString(undefined,{hour:'numeric',minute:'2-digit'})}</td>
      <td class="conv-name">${escapeHtml(conv.name)}</td>
      <td class="msg-text">${m.text ? renderMarkdownLite(m.text) : '<em style="color:var(--ink-faint);">(no text — attachment only)</em>'}</td>
      ${flagCells}
    </tr>`;
    return mainRow + renderReplyRow(m);
  }).join('');

  const approveHeader = state.showUser ? '<th></th>' : '';
  const flagHeaders = showFlagColumns ? `<th>All caps</th><th>Angry</th><th>Critical</th>${approveHeader}` : '';

  document.getElementById('reviewTable').innerHTML = `
    <table class="review">
      <thead><tr>
        <th>When</th><th>Conversation</th><th>Message</th>
        ${flagHeaders}
      </tr></thead>
      <tbody>${rows}</tbody>
    </table>`;

  if(showFlagColumns && state.showUser){
    document.querySelectorAll('.flag-checkbox input:not([disabled])').forEach(cb=>{
      cb.addEventListener('change', (e)=>{
        const id = e.target.dataset.id;
        const type = e.target.dataset.type;
        setRowOverrides(id, type, e.target.checked);
      });
    });
    document.querySelectorAll('.approve-btn').forEach(btn=>{
      btn.addEventListener('click', ()=> approveRow(btn.dataset.id));
    });
  }

  document.getElementById('pagination').innerHTML = `
    <button id="prevPage" ${reviewPage===0?'disabled':''}>Previous</button>
    <span>Page ${reviewPage+1} of ${totalPages}</span>
    <button id="nextPage" ${reviewPage>=totalPages-1?'disabled':''}>Next</button>`;
  document.getElementById('prevPage').addEventListener('click', ()=>{ reviewPage--; renderReviewTable(); });
  document.getElementById('nextPage').addEventListener('click', ()=>{ reviewPage++; renderReviewTable(); });

  if(reviewHighlightIds && reviewHighlightIds.length){
    const idsToHighlight = reviewHighlightIds;
    requestAnimationFrame(()=>{
      let firstRow = null;
      idsToHighlight.forEach(id=>{
        const row = document.querySelector(`tr[data-msg-id="${id}"]`);
        if(row){
          row.classList.add('row-highlight');
          if(!firstRow) firstRow = row;
        }
      });
      if(firstRow && typeof firstRow.scrollIntoView === 'function') firstRow.scrollIntoView({behavior:'smooth', block:'center'});
    });
    reviewHighlightIds = null;
  }
}

document.getElementById('reviewSearch').addEventListener('input', ()=>{ reviewPage = 0; renderReviewTable(); });
document.getElementById('reviewFilter').addEventListener('change', ()=>{ reviewPage = 0; renderReviewTable(); });
document.getElementById('exportAnnotatedBtn').addEventListener('click', exportAnnotatedConversations);

function onVisibilityToggleChanged(){
  state.showAuto = document.getElementById('toggleShowAuto').checked;
  state.showUser = document.getElementById('toggleShowUser').checked;
  attachFlags();
  renderCalendar();
  renderConvList(document.getElementById('convSearch').value);
  if(state.selectedConversation !== null) selectConversation(state.selectedConversation);
  renderReviewTable();
}
document.getElementById('toggleShowAuto').addEventListener('change', onVisibilityToggleChanged);
document.getElementById('toggleShowUser').addEventListener('change', onVisibilityToggleChanged);
document.getElementById('toggleShowReplies').addEventListener('change', ()=>{
  state.showReplies = document.getElementById('toggleShowReplies').checked;
  renderReviewTable();
});

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

function runAnalysis(name, opts){
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

document.querySelectorAll('.analytics-item').forEach(btn=>{
  btn.addEventListener('click', ()=> runAnalysis(btn.dataset.analysis, {}));
});

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

// =====================================================================
// Minimal SVG chart primitives (no external charting library — this page
// stays self-contained and works offline)
// =====================================================================

function renderBarChartSVG(container, bars, opts){
  opts = opts || {};
  const W = 700, H = 220, padL = 36, padR = 12, padT = 12, padB = 28;
  const plotW = W - padL - padR, plotH = H - padT - padB;
  const maxVal = Math.max(1, ...bars.map(b=>b.value));
  const barW = plotW / bars.length;

  let svg = `<svg class="chart-svg" viewBox="0 0 ${W} ${H}" style="width:100%; height:auto;">`;
  // gridlines
  for(let g=0; g<=4; g++){
    const y = padT + plotH - (g/4)*plotH;
    svg += `<line class="grid-line" x1="${padL}" y1="${y}" x2="${W-padR}" y2="${y}"/>`;
    svg += `<text x="${padL-6}" y="${y+3}" text-anchor="end">${Math.round(maxVal*g/4)}%</text>`;
  }
  bars.forEach((b, i) => {
    const x = padL + i*barW;
    const barH = (b.value / maxVal) * plotH;
    const y = padT + plotH - barH;
    svg += `<rect class="data-bar" x="${x+barW*0.15}" y="${y}" width="${barW*0.7}" height="${Math.max(barH,0.5)}"><title>${escapeHtml(b.tooltip||String(b.value))}</title></rect>`;
    if(b.label) svg += `<text x="${x+barW/2}" y="${H-8}" text-anchor="middle">${escapeHtml(b.label)}</text>`;
  });
  svg += `<line class="axis-line" x1="${padL}" y1="${padT+plotH}" x2="${W-padR}" y2="${padT+plotH}"/>`;
  svg += `</svg>`;
  container.innerHTML = svg;
}

function renderLineChartSVG(container, points, opts){
  opts = opts || {};
  const W = 760, H = 260, padL = 40, padR = 16, padT = 16, padB = 40;
  const plotW = W - padL - padR, plotH = H - padT - padB;
  if(points.length === 0){
    container.innerHTML = `<p class="hint">Not enough data yet.</p>`;
    return;
  }
  const maxVal = Math.max(1, ...points.map(p=>p.y));
  const stepX = points.length > 1 ? plotW / (points.length - 1) : 0;

  let svg = `<svg class="chart-svg" viewBox="0 0 ${W} ${H}" style="width:100%; height:auto;">`;
  for(let g=0; g<=4; g++){
    const y = padT + plotH - (g/4)*plotH;
    svg += `<line class="grid-line" x1="${padL}" y1="${y}" x2="${W-padR}" y2="${y}"/>`;
    svg += `<text x="${padL-6}" y="${y+3}" text-anchor="end">${Math.round(maxVal*g/4)}%</text>`;
  }
  const coords = points.map((p,i) => ({
    x: padL + i*stepX,
    y: padT + plotH - (p.y/maxVal)*plotH,
  }));
  const pathD = coords.map((c,i)=> (i===0?'M':'L') + c.x.toFixed(1) + ',' + c.y.toFixed(1)).join(' ');
  svg += `<path class="data-line" d="${pathD}"/>`;
  coords.forEach((c,i) => {
    svg += `<circle class="data-point" cx="${c.x}" cy="${c.y}" r="3"><title>${escapeHtml(opts.tooltipFn ? opts.tooltipFn(points[i]) : String(points[i].y))}</title></circle>`;
  });
  // x labels: show a subset to avoid crowding
  const labelEvery = Math.max(1, Math.ceil(points.length / 10));
  points.forEach((p,i) => {
    if(i % labelEvery === 0){
      svg += `<text x="${coords[i].x}" y="${H-16}" text-anchor="middle">${escapeHtml(p.x)}</text>`;
    }
  });
  svg += `<line class="axis-line" x1="${padL}" y1="${padT+plotH}" x2="${W-padR}" y2="${padT+plotH}"/>`;
  svg += `</svg>`;
  container.innerHTML = svg;
}

function renderScatterChartSVG(container, points, opts){
  opts = opts || {};
  const W = 720, H = 320, padL = 44, padR = 16, padT = 16, padB = 40;
  const plotW = W - padL - padR, plotH = H - padT - padB;
  if(points.length === 0){
    container.innerHTML = `<p class="hint">Not enough data yet.</p>`;
    return;
  }
  const useLog = opts.xScale === 'log';
  const xs = points.map(p => useLog ? Math.log10(p.x) : p.x);
  const minX = Math.min(...xs), maxX = Math.max(...xs);
  const maxY = Math.max(1, ...points.map(p=>p.y));
  const rangeX = (maxX - minX) || 1;

  let svg = `<svg class="chart-svg" viewBox="0 0 ${W} ${H}" style="width:100%; height:auto;">`;
  for(let g=0; g<=4; g++){
    const y = padT + plotH - (g/4)*plotH;
    svg += `<line class="grid-line" x1="${padL}" y1="${y}" x2="${W-padR}" y2="${y}"/>`;
    svg += `<text x="${padL-6}" y="${y+3}" text-anchor="end">${Math.round(maxY*g/4)}%</text>`;
  }
  points.forEach((p, i) => {
    const xv = useLog ? Math.log10(p.x) : p.x;
    const cx = padL + ((xv - minX) / rangeX) * plotW;
    const cy = padT + plotH - (p.y / maxY) * plotH;
    svg += `<circle class="data-point" cx="${cx.toFixed(1)}" cy="${cy.toFixed(1)}" r="4" style="cursor:pointer; opacity:0.75;" data-idx="${i}"><title>${escapeHtml(p.label||'')} — ${p.y.toFixed(1)}%</title></circle>`;
  });
  svg += `<text x="${padL}" y="${H-4}" text-anchor="start">${escapeHtml(opts.xLabel||'')}</text>`;
  svg += `<line class="axis-line" x1="${padL}" y1="${padT+plotH}" x2="${W-padR}" y2="${padT+plotH}"/>`;
  svg += `</svg>`;
  container.innerHTML = svg;

  if(opts.onPointClick){
    container.querySelectorAll('circle[data-idx]').forEach(el=>{
      el.addEventListener('click', ()=> opts.onPointClick(points[parseInt(el.dataset.idx,10)]));
    });
  }
}

// Last thing in the file, so everything it calls already exists. Deliberately
// not awaited: the load screen is already usable, and a slow or unreachable
// backend must not hold the page hostage while it decides there is nothing to
// restore.
tryRestoreSession();
