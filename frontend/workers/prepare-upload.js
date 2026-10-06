// The Web Worker that prepares a chosen file for sending (plan
// docs/plans/2026-10-06-load-only-what-the-page-shows.md §7b): a second
// thread the browser provides, so reading and slimming a large file never
// stops the page from moving its bar and clock. The page
// (infra/upload-preparer.js) posts it { file }; it answers with the
// messages workers/slim-stream.js describes. One worker prepares one file
// and is then ended by the page.

import { slimFile } from './slim-stream.js';

self.addEventListener('message', (event) => {
  slimFile(event.data.file, (message) => self.postMessage(message));
});
