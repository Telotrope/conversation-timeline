// After any change to flags or to the show switches, redraws the calendar,
// the conversation list, the open conversation and the chosen analysis from
// the sessions' counts. Review asks the server itself (ui/views/review.js).

import { state } from '../core/state.js';
import { rerunAnalysis } from './views/analytics.js';
import { renderCalendar } from './views/calendar.js';
import { renderConvList, selectConversation } from './views/conversations.js';

export function refreshAllViews(){
  renderCalendar();
  renderConvList(document.getElementById('convSearch').value);
  if(state.selectedConversation !== null) selectConversation(state.selectedConversation);
  rerunAnalysis();
}
