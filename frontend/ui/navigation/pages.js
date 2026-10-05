// Shows one page at a time: Sign-in, Upload, Describe or the timeline (plan
// docs/plans/2026-10-05-screen-flow.md §3). The page-level counterpart of
// switchTab in tabs.js: every page is a section of the one document, and
// showing one hides the rest. The account line shows on every page but
// Sign-in. Each change of page is recorded as a view, so the activity log
// says where you were when the tabs aren't shown.

import { viewEvent } from '../../core/activity-event.js';
import { noteMainShown, recordActivity } from '../../core/activity-sink.js';

const PAGES = Object.freeze({
  signIn: 'signInPage',
  upload: 'uploadPage',
  describe: 'describePage',
  timeline: 'mainContent',
});

let CURRENT = null;

export function showPage(name){
  if(!(name in PAGES)) throw new Error(`no page named ${JSON.stringify(name)}`);
  for(const [page, id] of Object.entries(PAGES)){
    document.getElementById(id).hidden = page !== name;
  }
  document.getElementById('accountBar').hidden = name === 'signIn';
  noteMainShown(name === 'timeline');
  if(name !== CURRENT) recordActivity(viewEvent(name, 'page'));
  CURRENT = name;
}

// The page shown now, or null before the first.
export function currentPage(){
  return CURRENT;
}
