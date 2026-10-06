// The reverse of ui/navigation/location.js: when the page loads or you press
// Back, reads the web address and opens the tab, conversation or analysis it
// names.

import { state } from '../core/state.js';
import { viewEvent } from '../core/activity-event.js';
import { recordActivity } from '../core/activity-sink.js';
import { whileApplyingHash } from './navigation/location.js';
import { switchTab } from './navigation/tabs.js';
import { runAnalysis } from './views/analytics.js';
import { selectConversation } from './views/conversations.js';
import { showReviewTab } from './views/review.js';

export function applyLocationHash(){
  const raw = window.location.hash.replace(/^#/, '');
  if(!raw) return;
  const [tab, arg] = raw.split('/');
  if(!document.getElementById('view-' + tab)) return;

  recordActivity(viewEvent(tab, 'router'));
  whileApplyingHash(()=>{
    switchTab(tab);
    if(tab === 'conversations' && arg !== undefined){
      const idx = parseInt(arg, 10);
      if(!isNaN(idx) && idx >= 0 && idx < state.conversations.length) selectConversation(idx);
    }
    if(tab === 'review') showReviewTab();
    if(tab === 'analytics' && arg){
      const btn = document.querySelector(`.analytics-item[data-analysis="${arg}"]`);
      if(btn) runAnalysis(arg, {});
    }
  });
}
