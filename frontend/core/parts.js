// Reads a request the server answers in parts (plan
// docs/plans/2026-10-06-load-only-what-the-page-shows.md §8c). Each part
// carries a cursor, sent back unchanged to get the next part (null in the
// last), and the user's data version. If the data version changes between
// two parts, the data changed underneath (an upload finished, a flag was
// saved elsewhere): the parts read so far may not fit together, so the
// reading starts again from the beginning, and the caller is told so it can
// say "Your data changed; starting again".

import { PageError } from './page-error.js';

// How many times a reading starts again before giving up: data that keeps
// changing would otherwise keep the page reading forever.
export const MAX_RESTARTS = 3;

// fetchPart(cursor): resolves to one part (cursor null for the first).
// onPart(part, partsSoFar): after each part, for the progress bar.
// onRestart(): the data changed and the reading starts again.
// Resolves to { parts, dataVersion }.
export async function readAllParts(fetchPart, { onPart = () => {}, onRestart = () => {} } = {}){
  for(let attempt = 0; ; attempt += 1){
    const reading = await readOnce(fetchPart, onPart);
    if(reading) return reading;
    if(attempt >= MAX_RESTARTS){
      throw new PageError('your data kept changing while it was being read; try again in a moment', 'other');
    }
    onRestart();
  }
}

// One reading from the start; null when the data version changed part-way.
async function readOnce(fetchPart, onPart){
  const parts = [];
  let cursor = null;
  do{
    const part = await fetchPart(cursor);
    if(parts.length > 0 && part.data_version !== parts[0].data_version) return null;
    parts.push(part);
    onPart(part, parts);
    cursor = part.cursor ?? null;
  } while(cursor !== null);
  return { parts, dataVersion: parts[0].data_version };
}

// Every item of `field` across `parts`, in order.
export function joinParts(parts, field){
  return parts.flatMap((part) => part[field]);
}
