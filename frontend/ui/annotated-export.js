// The download button: saves every conversation with both the automatic
// flags and your corrections written into each message, in a file this
// page can load again. The server builds it from the stored messages, in
// parts (plan docs/plans/2026-10-06-load-only-what-the-page-shows.md §5,
// §8c): each part's text is kept as it arrives and the parts are joined into
// one file once the last arrives, with the bar showing sessions done of the
// total. If the data changes part-way, the download starts again.

import { errorKindOf, errorStatusOf } from '../core/page-error.js';
import { readAllParts } from '../core/parts.js';
import { ensureAuthToken, fetchExportPart } from '../infra/api-client.js';
import { createProgressBar, setSaveStatus } from './widgets/status-indicators.js';

export const DOWNLOAD_NAME = 'conversations-with-flags.json';

const BAR = createProgressBar({
  nodes: () => ({ fill: document.getElementById('exportProgressFill'), label: document.getElementById('exportProgressLabel') }),
});

// Resolves once the file is saved, or the failure shown on the save line.
export async function exportAnnotatedConversations(){
  const button = document.getElementById('exportAnnotatedBtn');
  const wrap = document.getElementById('exportProgress');
  button.disabled = true;
  wrap.hidden = false;
  BAR.working('progress.exporting', { done: 0, total: 0 });
  try{
    const token = await ensureAuthToken(document.getElementById('devLoginSub').value.trim());
    const { parts } = await readAllParts((cursor) => fetchExportPart(token, cursor), {
      onPart: (part) => BAR.measured('progress.exporting', {}, part.sessions_done, part.sessions_total),
      onRestart: () => BAR.working('progress.data_changed'),
    });
    save(new Blob(parts.map((p) => p.part), { type: 'application/json' }));
    setSaveStatus('export.downloaded');
    wrap.hidden = true;
  } catch(err){
    console.error(err);
    BAR.failed('progress.request_failed', { detail: err.message });
    setSaveStatus('export.failed', { detail: err.message, status: errorStatusOf(err), error_kind: errorKindOf(err) });
  } finally {
    BAR.stop();
    button.disabled = false;
  }
}

function save(blob){
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = DOWNLOAD_NAME;
  a.click();
  URL.revokeObjectURL(url);
}
