// Shows a stored file's text on the page by its kind (plan
// docs/plans/2026-10-06-load-only-what-the-page-shows.md §4), never running
// any code it contains (core/file-kinds.js says which way each kind is
// shown):
// - an SVG as an image element, which runs no scripts inside the drawing;
// - a web page or widget laid out in a frame whose `sandbox` attribute is
//   empty, which turns every permission off: no scripts, no pop-ups, no
//   forms, no reaching this page; "Show source" switches to its text;
// - Markdown formatted by the page's own renderer, which escapes it first;
// - code coloured by highlight.js (BSD-3-Clause, vendor/highlight.js, the
//   global `hljs`), which only colours text and escapes it;
// - anything else as plain text.

import { fileView } from '../../core/file-kinds.js';
import { escapeHtml, renderMarkdownLite } from './markup.js';

// Code as coloured HTML, or escaped plain text when highlight.js isn't
// loaded or doesn't know the language.
export function codeHtml(text, language){
  const hljs = globalThis.hljs;
  if(hljs && language && hljs.getLanguage(language)) return hljs.highlight(text, { language, ignoreIllegals: true }).value;
  return escapeHtml(text);
}

function codeBlock(text, language){
  const pre = document.createElement('pre');
  pre.className = 'file-code hljs';
  const code = document.createElement('code');
  code.innerHTML = codeHtml(text, language);
  pre.appendChild(code);
  return pre;
}

// Fills `body` with the file. `objectUrl(blob)` makes an address for a
// drawing (the caller revokes it when the viewer closes). Returns
// { source(shown) } for a web page, which switches between the page and its
// source, or null for every other kind.
export function showFileIn(body, text, kind, objectUrl){
  const view = fileView(kind);
  body.replaceChildren();
  if(view.mode === 'image'){
    const img = document.createElement('img');
    img.className = 'file-image';
    img.alt = 'the drawing';
    img.src = objectUrl(new Blob([text], { type: view.mime }));
    body.appendChild(img);
    return null;
  }
  if(view.mode === 'frame'){
    const frame = document.createElement('iframe');
    frame.className = 'file-frame';
    frame.setAttribute('sandbox', '');
    frame.setAttribute('referrerpolicy', 'no-referrer');
    frame.title = 'the page, with its scripts off';
    frame.srcdoc = text;
    const source = codeBlock(text, 'xml');
    source.hidden = true;
    body.append(frame, source);
    return { source(shown){ frame.hidden = shown; source.hidden = !shown; } };
  }
  if(view.mode === 'markdown'){
    const div = document.createElement('div');
    div.className = 'file-markdown';
    div.innerHTML = renderMarkdownLite(text);
    body.appendChild(div);
    return null;
  }
  if(view.mode === 'code'){
    body.appendChild(codeBlock(text, view.language));
    return null;
  }
  const pre = document.createElement('pre');
  pre.className = 'file-text';
  pre.textContent = text;
  body.appendChild(pre);
  return null;
}
