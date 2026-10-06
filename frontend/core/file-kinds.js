// How each kind of stored file is shown (plan
// docs/plans/2026-10-06-load-only-what-the-page-shows.md §4), never running
// any code a file contains: an SVG as a picture (an image element runs no
// scripts inside it), a web page or widget in a frame with every permission
// off, Markdown formatted by the page's own renderer, code coloured by
// highlight.js (which only colours text), and anything else as text.

const VIEWS = Object.freeze({
  svg: { mode: 'image', mime: 'image/svg+xml', label: 'drawing' },
  web_page: { mode: 'frame', mime: 'text/html', label: 'web page' },
  markdown: { mode: 'markdown', mime: 'text/markdown', label: 'Markdown' },
  code: { mode: 'code', mime: 'text/plain', label: 'code' },
  text: { mode: 'text', mime: 'text/plain', label: 'text' },
});
const OTHER = Object.freeze({ mode: 'text', mime: 'text/plain', label: 'file' });

// The way to show a file of `kind` (a FileRef's kind: { kind, language? }):
// { mode: 'image'|'frame'|'markdown'|'code'|'text', mime, label, language }.
// `language` is a highlight.js language name, for code only; `label` names
// the kind on the file's card. Kinds the page doesn't know are text.
export function fileView(kind){
  const view = Object.prototype.hasOwnProperty.call(VIEWS, kind.kind) ? VIEWS[kind.kind] : OTHER;
  if(view.mode !== 'code') return { ...view, language: null };
  const language = typeof kind.language === 'string' && kind.language ? kind.language : null;
  return { ...view, language, label: language ? `${language} code` : view.label };
}
