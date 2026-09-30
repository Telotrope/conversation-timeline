// Makes text safe to put into the page, and renders message text written in
// Markdown (headings, bold, lists, code) as HTML. The text is escaped first,
// so a message cannot inject its own HTML.

export function escapeHtml(s){
  const div = document.createElement('div');
  div.textContent = s;
  return div.innerHTML;
}

// A small, dependency-free markdown renderer for displaying message text —
// handles what actually shows up in real conversations (headers, bold,
// italic, inline code, bullet/numbered lists, paragraph breaks) without
// pulling in a markdown library for a self-contained offline page. Escapes
// the raw text FIRST, then only ever adds tags on top of that escaped
// text — so nothing in the original message can inject real HTML.
export function renderMarkdownLite(text){
  if(!text) return '';
  const escaped = escapeHtml(text);

  function inlineFormat(line){
    line = line.replace(/\*\*(.+?)\*\*/g, '<strong>$1</strong>');
    line = line.replace(/__(.+?)__/g, '<strong>$1</strong>');
    line = line.replace(/(^|[^*])\*([^*\n]+)\*(?!\*)/g, '$1<em>$2</em>');
    line = line.replace(/(^|[^_])_([^_\n]+)_(?!_)/g, '$1<em>$2</em>');
    line = line.replace(/`([^`]+)`/g, '<code style="background:var(--paper); padding:1px 4px; border-radius:3px; font-size:0.9em;">$1</code>');
    return line;
  }

  const lines = escaped.split('\n');
  let html = '';
  let inUl = false, inOl = false;
  let paragraphBuffer = [];

  function flushParagraph(){
    if(paragraphBuffer.length){
      html += '<p style="margin:0 0 8px;">' + paragraphBuffer.join('<br>') + '</p>';
      paragraphBuffer = [];
    }
  }
  function closeLists(){
    if(inUl){ html += '</ul>'; inUl = false; }
    if(inOl){ html += '</ol>'; inOl = false; }
  }

  lines.forEach(line => {
    const trimmed = line.trim();

    if(trimmed === ''){
      flushParagraph();
      closeLists();
      return;
    }

    const headerMatch = trimmed.match(/^(#{1,6})\s+(.*)$/);
    if(headerMatch){
      flushParagraph();
      closeLists();
      const size = Math.max(0.85, 1.15 - headerMatch[1].length * 0.08);
      html += `<div style="font-weight:600; font-size:${size}rem; margin:8px 0 4px;">${inlineFormat(headerMatch[2])}</div>`;
      return;
    }

    const ulMatch = trimmed.match(/^[-*]\s+(.*)$/);
    if(ulMatch){
      flushParagraph();
      if(inOl){ html += '</ol>'; inOl = false; }
      if(!inUl){ html += '<ul style="margin:2px 0 8px; padding-left:20px;">'; inUl = true; }
      html += `<li>${inlineFormat(ulMatch[1])}</li>`;
      return;
    }

    const olMatch = trimmed.match(/^\d+\.\s+(.*)$/);
    if(olMatch){
      flushParagraph();
      if(inUl){ html += '</ul>'; inUl = false; }
      if(!inOl){ html += '<ol style="margin:2px 0 8px; padding-left:20px;">'; inOl = true; }
      html += `<li>${inlineFormat(olMatch[1])}</li>`;
      return;
    }

    closeLists();
    paragraphBuffer.push(inlineFormat(line));
  });

  flushParagraph();
  closeLists();
  return html || escaped;
}
