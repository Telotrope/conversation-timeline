// The annotated download with nothing loaded (ui/annotated-export.js). The
// page can't reach this -- the button sits in the main view, shown only
// after a load, and nothing clears the loaded data -- so the function is
// called directly (plan docs/plans/2026-10-05-page-coverage-gaps.md).

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { exportAnnotatedConversations } from '../ui/annotated-export.js';
import { state } from '../core/state.js';
import { connectActivitySink } from '../core/activity-sink.js';
import { PAGE_MESSAGES } from '../ui/widgets/page-messages.js';

test('with nothing loaded, it says so on the save line and downloads nothing', () => {
  const saveLine = { textContent: '' };
  globalThis.document = {
    getElementById: (id) => (id === 'saveStatus' ? saveLine : null),
    createElement: () => { throw new Error('nothing should be downloaded'); },
  };
  const events = [];
  connectActivitySink({ record: (e) => events.push(e), requestStarted(){}, requestFinished(){} });
  state.rawData = null;

  exportAnnotatedConversations();

  connectActivitySink(null);
  assert.equal(saveLine.textContent, PAGE_MESSAGES['export.no_data'].text({}));
  assert.equal(events.length, 1);
  assert.equal(events[0].message, 'export.no_data');
});
