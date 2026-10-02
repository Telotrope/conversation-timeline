// Turns numbers and timestamps into short readable text: file sizes, time
// remaining, durations, clock times, and day and month headings.

export function formatBytes(n){
  if(n < 1024) return n + ' B';
  if(n < 1024 * 1024) return (n / 1024).toFixed(0) + ' KB';
  return (n / (1024 * 1024)).toFixed(1) + ' MB';
}

// Deliberately coarse. The estimate is derived from a few seconds of
// observed throughput, so rendering it to a tenth of a second would claim a
// precision it does not have.
export function formatEta(seconds){
  if(!isFinite(seconds) || seconds < 0) return '';
  if(seconds < 5) return 'almost done';
  if(seconds < 60) return `about ${Math.round(seconds / 5) * 5} seconds left`;
  if(seconds < 120) return 'about a minute left';
  return `about ${Math.round(seconds / 60)} minutes left`;
}

export function formatDuration(sec){
  if(sec < 60) return sec + 's';
  const h = Math.floor(sec/3600);
  const m = Math.floor((sec%3600)/60);
  const s = sec%60;
  if(h > 0) return `${h}h ${m}m`;
  return `${m}m ${s}s`;
}

export function formatClock(iso){
  return new Date(iso).toLocaleTimeString(undefined, {hour:'numeric', minute:'2-digit'});
}

export function formatDayHeading(dateStr){
  const d = new Date(dateStr + 'T00:00:00');
  return d.toLocaleDateString(undefined, {weekday:'long', month:'long', day:'numeric', year:'numeric'});
}

export function formatMonthHeading(dateStr){
  const d = new Date(dateStr + 'T00:00:00');
  return d.toLocaleDateString(undefined, {month:'long', year:'numeric'});
}
