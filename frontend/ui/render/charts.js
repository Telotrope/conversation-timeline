// Draws the bar, line and scatter charts the Analytics tab uses, as SVG
// inside a given element.

import { escapeHtml } from './markup.js';

// =====================================================================
// Minimal SVG chart primitives (no external charting library — this page
// stays self-contained and works offline)
// =====================================================================

export function renderBarChartSVG(container, bars, opts){
  opts = opts || {};
  const W = 700, H = 220, padL = 36, padR = 12, padT = 12, padB = 28;
  const plotW = W - padL - padR, plotH = H - padT - padB;
  const maxVal = Math.max(1, ...bars.map(b=>b.value));
  const barW = plotW / bars.length;

  let svg = `<svg class="chart-svg" viewBox="0 0 ${W} ${H}" style="width:100%; height:auto;">`;
  // gridlines
  for(let g=0; g<=4; g++){
    const y = padT + plotH - (g/4)*plotH;
    svg += `<line class="grid-line" x1="${padL}" y1="${y}" x2="${W-padR}" y2="${y}"/>`;
    svg += `<text x="${padL-6}" y="${y+3}" text-anchor="end">${Math.round(maxVal*g/4)}%</text>`;
  }
  bars.forEach((b, i) => {
    const x = padL + i*barW;
    const barH = (b.value / maxVal) * plotH;
    const y = padT + plotH - barH;
    svg += `<rect class="data-bar" x="${x+barW*0.15}" y="${y}" width="${barW*0.7}" height="${Math.max(barH,0.5)}"><title>${escapeHtml(b.tooltip||String(b.value))}</title></rect>`;
    if(b.label) svg += `<text x="${x+barW/2}" y="${H-8}" text-anchor="middle">${escapeHtml(b.label)}</text>`;
  });
  svg += `<line class="axis-line" x1="${padL}" y1="${padT+plotH}" x2="${W-padR}" y2="${padT+plotH}"/>`;
  svg += `</svg>`;
  container.innerHTML = svg;
}

export function renderLineChartSVG(container, points, opts){
  opts = opts || {};
  const W = 760, H = 260, padL = 40, padR = 16, padT = 16, padB = 40;
  const plotW = W - padL - padR, plotH = H - padT - padB;
  if(points.length === 0){
    container.innerHTML = `<p class="hint">Not enough data yet.</p>`;
    return;
  }
  const maxVal = Math.max(1, ...points.map(p=>p.y));
  const stepX = points.length > 1 ? plotW / (points.length - 1) : 0;

  let svg = `<svg class="chart-svg" viewBox="0 0 ${W} ${H}" style="width:100%; height:auto;">`;
  for(let g=0; g<=4; g++){
    const y = padT + plotH - (g/4)*plotH;
    svg += `<line class="grid-line" x1="${padL}" y1="${y}" x2="${W-padR}" y2="${y}"/>`;
    svg += `<text x="${padL-6}" y="${y+3}" text-anchor="end">${Math.round(maxVal*g/4)}%</text>`;
  }
  const coords = points.map((p,i) => ({
    x: padL + i*stepX,
    y: padT + plotH - (p.y/maxVal)*plotH,
  }));
  const pathD = coords.map((c,i)=> (i===0?'M':'L') + c.x.toFixed(1) + ',' + c.y.toFixed(1)).join(' ');
  svg += `<path class="data-line" d="${pathD}"/>`;
  coords.forEach((c,i) => {
    svg += `<circle class="data-point" cx="${c.x}" cy="${c.y}" r="3"><title>${escapeHtml(opts.tooltipFn ? opts.tooltipFn(points[i]) : String(points[i].y))}</title></circle>`;
  });
  // x labels: show a subset to avoid crowding
  const labelEvery = Math.max(1, Math.ceil(points.length / 10));
  points.forEach((p,i) => {
    if(i % labelEvery === 0){
      svg += `<text x="${coords[i].x}" y="${H-16}" text-anchor="middle">${escapeHtml(p.x)}</text>`;
    }
  });
  svg += `<line class="axis-line" x1="${padL}" y1="${padT+plotH}" x2="${W-padR}" y2="${padT+plotH}"/>`;
  svg += `</svg>`;
  container.innerHTML = svg;
}

export function renderScatterChartSVG(container, points, opts){
  opts = opts || {};
  const W = 720, H = 320, padL = 44, padR = 16, padT = 16, padB = 40;
  const plotW = W - padL - padR, plotH = H - padT - padB;
  if(points.length === 0){
    container.innerHTML = `<p class="hint">Not enough data yet.</p>`;
    return;
  }
  const useLog = opts.xScale === 'log';
  const xs = points.map(p => useLog ? Math.log10(p.x) : p.x);
  const minX = Math.min(...xs), maxX = Math.max(...xs);
  const maxY = Math.max(1, ...points.map(p=>p.y));
  const rangeX = (maxX - minX) || 1;

  let svg = `<svg class="chart-svg" viewBox="0 0 ${W} ${H}" style="width:100%; height:auto;">`;
  for(let g=0; g<=4; g++){
    const y = padT + plotH - (g/4)*plotH;
    svg += `<line class="grid-line" x1="${padL}" y1="${y}" x2="${W-padR}" y2="${y}"/>`;
    svg += `<text x="${padL-6}" y="${y+3}" text-anchor="end">${Math.round(maxY*g/4)}%</text>`;
  }
  points.forEach((p, i) => {
    const xv = useLog ? Math.log10(p.x) : p.x;
    const cx = padL + ((xv - minX) / rangeX) * plotW;
    const cy = padT + plotH - (p.y / maxY) * plotH;
    svg += `<circle class="data-point" cx="${cx.toFixed(1)}" cy="${cy.toFixed(1)}" r="4" style="cursor:pointer; opacity:0.75;" data-idx="${i}"><title>${escapeHtml(p.label||'')} — ${p.y.toFixed(1)}%</title></circle>`;
  });
  svg += `<text x="${padL}" y="${H-4}" text-anchor="start">${escapeHtml(opts.xLabel||'')}</text>`;
  svg += `<line class="axis-line" x1="${padL}" y1="${padT+plotH}" x2="${W-padR}" y2="${padT+plotH}"/>`;
  svg += `</svg>`;
  container.innerHTML = svg;

  if(opts.onPointClick){
    container.querySelectorAll('circle[data-idx]').forEach(el=>{
      el.addEventListener('click', ()=> opts.onPointClick(points[parseInt(el.dataset.idx,10)]));
    });
  }
}
