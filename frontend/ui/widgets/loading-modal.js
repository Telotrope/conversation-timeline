// The loading modal: "Loading your timeline", in front of the timeline while
// it downloads, blocking clicks on the page behind it (plan
// docs/plans/2026-10-05-screen-flow.md §6b).
//
// It has no bar of its own. The page has one status line and one progress
// bar (#loadProgressGroup), and every function in status-indicators.js works
// on those; the Upload page and the modal are never shown together, so the
// modal borrows them while it is open and puts them back when it closes.

let HOME = null;

function group(){
  return document.getElementById('loadProgressGroup');
}

export function openLoadingModal(){
  const g = group();
  if(!HOME) HOME = { parent: g.parentNode, next: g.nextSibling };
  document.getElementById('loadingModalSlot').appendChild(g);
  document.getElementById('loadingModalActions').hidden = true;
  document.getElementById('loadingModal').hidden = false;
}

export function closeLoadingModal(){
  if(HOME){
    HOME.parent.insertBefore(group(), HOME.next);
    HOME = null;
  }
  document.getElementById('loadingModal').hidden = true;
}

// After a failure, already shown on the borrowed status line: offers Try
// again and Sign out.
export function offerLoadingModalChoices(onRetry, onSignOut){
  document.getElementById('loadingModalActions').hidden = false;
  document.getElementById('loadingRetryBtn').onclick = onRetry;
  document.getElementById('loadingSignOutBtn').onclick = onSignOut;
}
