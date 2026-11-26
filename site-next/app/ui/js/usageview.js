import { h, svg } from './dom.js';
import { duration, kindColor, kindMark, label, usageColor } from './format.js';
import { t } from './i18n.js';
import { providerIcon } from './icons.js';

const RING_CENTER = 18;
const RING_OUTER = 16.5;
const RING_STEP = 4.5;
const USAGE_RINGS = 2;

const usedPercent = (window) => Math.max(0, Math.min(100, window.used));

function ringCircles(window, index) {
  const radius = RING_OUTER - index * RING_STEP;
  const circle = `cx="${RING_CENTER}" cy="${RING_CENTER}" r="${radius}"`;
  const used = usedPercent(window);
  const dash = used < 100 ? ` pathLength="100" stroke-dasharray="${used} 100"` : '';
  return `<circle class="ring-track" ${circle}/><circle class="ring-fill" ${circle}${dash} style="stroke:${usageColor(window.used)}"/>`;
}

export function usageRing(entry) {
  const icon = providerIcon(entry.kind);
  const rings = entry.windows.slice(0, USAGE_RINGS).map(ringCircles).join('');
  const ring = h(
    'div',
    { class: 'usage-ring', role: 'img', dataset: { kind: entry.kind } },
    svg(`<svg class="ring" width="36" height="36" viewBox="0 0 36 36" aria-hidden="true">${rings}</svg>`),
    h('span', { class: 'usage-ring-icon' }, icon ? svg(icon) : h('span', { class: 'summary-mark', style: `color:${kindColor(entry.kind)}` }, kindMark(entry.kind))),
  );
  return ring;
}

export function ringTip(entry, at) {
  const lines = entry.windows.map((window) => {
    const resets = window.resetsAt ? ` · ${t('resets in {time}', { time: duration(window.resetsAt - at) })}` : '';
    return `${t(window.label)} ${Math.round(window.used)}%${resets}`;
  });
  if (entry.credits !== null && entry.credits !== undefined) lines.push(t('funds {amount}', { amount: `$${entry.credits.toFixed(2)}` }));
  if (entry.problem) lines.push(t(entry.problem));
  return [label(entry.kind), ...lines].join('\n');
}

