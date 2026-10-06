// Places citation markers in a reply's text (plan
// docs/plans/2026-10-06-load-only-what-the-page-shows.md §4c). A citation
// marks a span of the text by its start and end positions and names the
// address it came from; Review shows a small numbered link after each cited
// span. The positions count the raw text, before Markdown formatting, so the
// markers go in first, as characters formatting leaves alone, and are
// turned into links once the text is formatted (ui/render/reply-markup.js).

// A marker is OPEN, the citation's number, CLOSE: two characters from
// Unicode's private-use area, which no font draws and no formatting touches.
export const MARK_OPEN = '';
export const MARK_CLOSE = '';
export const MARK = /(\d+)/g;
const MARK_CHARS = /[]/g;

// The text with a marker after each cited span, and the sources by number:
// { text, sources: [{ number, address: { kind, address } }] }. Citations are
// numbered from 1 in the order their spans start. Marker characters already
// in the text become U+FFFD, one for one, so they can't pose as markers and
// every position still counts the same characters. A position past the end
// is taken as the end.
export function withCitationMarks(text, citations){
  const clean = text.replace(MARK_CHARS, '�');
  const numbered = citations
    .map((c, i) => ({ ...c, i }))
    .sort((a, b) => a.start - b.start || a.end - b.end || a.i - b.i)
    .map((c, n) => ({ ...c, number: n + 1, at: Math.min(Math.max(c.end, 0), clean.length) }));
  let marked = clean;
  for(const c of [...numbered].sort((a, b) => b.at - a.at || b.number - a.number)){
    marked = marked.slice(0, c.at) + MARK_OPEN + c.number + MARK_CLOSE + marked.slice(c.at);
  }
  return { text: marked, sources: numbered.map((c) => ({ number: c.number, address: c.address })) };
}

// Whether a cited address may become a link: only a web address the server
// marked as one, and only http or https, so no other kind of link (such as
// one that would run script) is ever made.
export function isWebAddress(address){
  return address.kind === 'web' && /^https?:\/\//i.test(address.address);
}
