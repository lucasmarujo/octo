import { h } from './dom.js';

export const MOODS = ['idle', 'working', 'needs', 'done', 'lost'];
export const SLEEP = 'sleep';

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
