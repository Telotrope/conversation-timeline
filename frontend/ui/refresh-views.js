// After any change to flags, recomputes them and redraws the calendar, the
// conversation list, the open conversation, the review table and the chosen
// analysis.

import { attachFlags } from '../core/flags.js';
import { state } from '../core/state.js';
import { rerunAnalysis } from './views/analytics.js';
import { renderCalendar } from './views/calendar.js';
import { renderConvList, selectConversation } from './views/conversations.js';
import { renderReviewTable } from './views/review.js';

// After any change to flags or to the show switches: recompute which
// messages count as flagged, then redraw every view that shows them.
export function refreshAllViews(){
  attachFlags();
  renderCalendar();
  renderConvList(document.getElementById('convSearch').value);
  if(state.selectedConversation !== null) selectConversation(state.selectedConversation);
  renderReviewTable();
  rerunAnalysis();
}
