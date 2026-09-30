// The page's starting point: connects every button, box and switch in the
// markup to the code that handles it, hands the flag-edit handlers to the
// review table, then tries to restore your last session. Nothing else lives
// here.

import { buildBlocks } from './core/blocks.js';
import { attachFlags } from './core/flags.js';
import { state } from './core/state.js';
import { clearAuthToken } from './infra/api-client.js';
import { exportAnnotatedConversations } from './ui/annotated-export.js';
import { approveRow, onVisibilityToggleChanged, setRowOverrides } from './ui/flag-edits.js';
import { handleLoadClick, tryRestoreSession } from './ui/load-flow.js';
import { rememberLocation } from './ui/navigation/location.js';
import { switchTab } from './ui/navigation/tabs.js';
import { applyLocationHash } from './ui/router.js';
import { runAnalysis } from './ui/views/analytics.js';
import { renderConvList } from './ui/views/conversations.js';
import { renderReviewTable, setFlagEditHandlers, showFirstReviewPage } from './ui/views/review.js';
import { setLoadStatus } from './ui/widgets/status-indicators.js';

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

// Nothing renders until a conversations.json file is loaded via the load
// screen (see handleLoadClick in ui/load-flow.js), which populates
// state.conversations/state.messages/state.humanMessages and then triggers
// the first render itself.
state.blocks = buildBlocks();
attachFlags();

window.addEventListener('hashchange', applyLocationHash);
document.querySelectorAll('nav.tabs button').forEach(b=>{
  b.addEventListener('click', ()=>{ switchTab(b.dataset.tab); rememberLocation(); });
});

document.getElementById('convSearch').addEventListener('input', (e)=> renderConvList(e.target.value));

document.getElementById('reviewSearch').addEventListener('input', showFirstReviewPage);
document.getElementById('reviewFilter').addEventListener('change', showFirstReviewPage);
setFlagEditHandlers(setRowOverrides, approveRow);
document.getElementById('exportAnnotatedBtn').addEventListener('click', exportAnnotatedConversations);
document.getElementById('toggleShowAuto').addEventListener('change', onVisibilityToggleChanged);
document.getElementById('toggleShowUser').addEventListener('change', onVisibilityToggleChanged);
document.getElementById('toggleShowReplies').addEventListener('change', ()=>{
  state.showReplies = document.getElementById('toggleShowReplies').checked;
  renderReviewTable();
});

document.querySelectorAll('.analytics-item').forEach(btn=>{
  btn.addEventListener('click', ()=> runAnalysis(btn.dataset.analysis, {}));
});

// Last thing in the file, so everything it calls already exists. Deliberately
// not awaited: the load screen is already usable, and a slow or unreachable
// backend must not hold the page hostage while it decides there is nothing to
// restore.
tryRestoreSession();
