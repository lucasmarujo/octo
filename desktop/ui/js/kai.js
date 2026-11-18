import { blend } from './accent.js';
import { h } from './dom.js';

export const MOODS = ['idle', 'working', 'needs', 'done', 'lost'];
export const SLEEP = 'sleep';

const STRIPS = [...MOODS, SLEEP];
const KAI_BODY = '#e85eb6';
const WHITE = '#ffffff';
const BLACK = '#000000';

let strips = null;
let painting = 0;

function sprite(mood, extra) {
  return h('span', { class: `kai${extra ? ` ${extra}` : ''}`, dataset: { mood }, 'aria-hidden': 'true' }, h('span', { class: 'kai-strip' }));
}

export function glyph(mood = 'idle') {
  return sprite(mood);
}

export function sleeping() {
  return sprite(SLEEP, 'kai-sleeping');
}

export function moodOf(agents) {
  const live = agents.filter((agent) => !agent.starting);
  if (live.some((agent) => agent.state === 'paused')) return 'needs';
  if (agents.some((agent) => agent.state === 'working' || agent.starting)) return 'working';
  if (live.some((agent) => agent.state === 'done')) return 'done';
  if (live.some((agent) => agent.state === 'lost')) return 'lost';
  return 'idle';
}

export function kaiTones(accent) {
  return new Map([
    [KAI_BODY, accent],
    ['#f4e4f0', blend(accent, WHITE, 0.8)],
    ['#f9d3e8', blend(accent, WHITE, 0.6)],
    ['#f2899a', blend(accent, WHITE, 0.3)],
    ['#f08acb', blend(accent, WHITE, 0.2)],
    ['#c84898', blend(accent, BLACK, 0.2)],
    ['#8f3170', blend(accent, BLACK, 0.45)],
    ['#2e0b29', blend(accent, BLACK, 0.7)],
    ['#20061f', blend(accent, BLACK, 0.8)],
  ]);
}

export function recolorStrip(markup, tones) {
  return markup.replace(/#[0-9a-f]{6}\b/gi, (color) => tones.get(color.toLowerCase()) ?? color);
}

function stripUrl(markup) {
  return `url("data:image/svg+xml,${encodeURIComponent(markup)}")`;
}

function loadStrips() {
  strips ??= Promise.all(STRIPS.map((mood) => fetch(`kai/${mood}.svg`).then((response) => response.text()))).catch((err) => {
    strips = null;
    throw err;
  });
  return strips;
}

export async function paintKai(accent) {
  const turn = ++painting;
  const root = document.documentElement;
  if (!accent || accent === KAI_BODY) {
    for (const mood of STRIPS) root.style.removeProperty(`--kai-${mood}`);
    return;
  }
  let sources;
  try {
    sources = await loadStrips();
  } catch (err) {
    return;
  }
  if (turn !== painting) return;
  const tones = kaiTones(accent);
  STRIPS.forEach((mood, index) => root.style.setProperty(`--kai-${mood}`, stripUrl(recolorStrip(sources[index], tones))));
}
