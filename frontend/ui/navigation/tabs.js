// Switches between the Calendar, Conversations, "Review & flags" and
// Analytics tabs: highlights the chosen tab button, shows that tab's view and
// hides the others.

import { noteTabShown } from '../../core/activity-sink.js';

// --- Tabs ---
export function switchTab(name){
  noteTabShown(name);
  document.querySelectorAll('nav.tabs button').forEach(b=>{
    b.classList.toggle('active', b.dataset.tab === name);
  });
  document.querySelectorAll('.view').forEach(v=>{
    v.classList.toggle('active', v.id === 'view-' + name);
  });
}
