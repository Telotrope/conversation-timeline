// Records where you are in the web address, such as #conversations/3, so the
// browser's Back button returns to the previous view instead of leaving the
// page, and a reload reopens the same view. Reading the address back is
// ui/router.js's job.

import { state } from '../../core/state.js';

// --- Where you are, in the URL ---
// Without this, every tab change and conversation selection is invisible to
// the browser, so Back leaves the page entirely and the whole export has to
// be loaded again. The three axes worth remembering are the tab, the open
// conversation, and the chosen analysis.
//
// A hash, not history.pushState. The original reason no longer holds: the
// page used to be opened as a file:// URL, where some browsers restrict
// pushState, but it is now only served over HTTP. Switching would change
// behavior, so it is left for a separate decision.
//
// Review search and pagination deliberately stay out -- they change on every
// keystroke and would bury real navigation under dozens of history entries.

// Set while applying a hash, so restoring state doesn't immediately write
// the same hash back and fight with the browser's own history.
let APPLYING_HASH = false;

// Runs fn with the guard above set, clearing it however fn exits, so no other
// module can set the flag and forget to clear it.
export function whileApplyingHash(fn){
  APPLYING_HASH = true;
  try{
    fn();
  } finally {
    APPLYING_HASH = false;
  }
}

function currentLocationHash(){
  if(state.selectedAnalysis && document.getElementById('view-analytics').classList.contains('active')){
    return `#analytics/${state.selectedAnalysis}`;
  }
  const active = document.querySelector('nav.tabs button.active');
  const tab = active ? active.dataset.tab : 'calendar';
  if(tab === 'conversations' && state.selectedConversation !== null) return `#conversations/${state.selectedConversation}`;
  return `#${tab}`;
}

export function rememberLocation(){
  if(APPLYING_HASH) return;
  const next = currentLocationHash();
  if(next !== window.location.hash) window.location.hash = next;
}
