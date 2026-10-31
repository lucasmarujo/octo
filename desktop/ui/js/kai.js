import { h, svg } from './dom.js';

const PINK = '#FF5F87';
const SPOT = '#E2486F';
const WHITE = '#ECECF2';
const INK = '#121218';
const NAVY = '#2C3C78';

const LEG = `stroke="${PINK}" stroke-width="6.5" stroke-linecap="round"`;
const SLEEP_LEG = `stroke="${PINK}" stroke-width="13" stroke-linecap="round"`;

export const LAYERS = [
  { name: 'leg-1', markup: `<path d="M22 38C18 45 14 47 12 53" ${LEG}/>` },
  { name: 'leg-2', markup: `<path d="M32 41V55" ${LEG}/>` },
  { name: 'leg-3', markup: `<path d="M42 38C46 45 50 47 52 53" ${LEG}/>` },
  { name: 'head', markup: `<ellipse cx="32" cy="29" rx="17" ry="16" fill="${PINK}"/>` },
  { name: 'eyes', markup: `<circle cx="26" cy="30" r="4.4" fill="${WHITE}"/><circle cx="38" cy="30" r="4.4" fill="${WHITE}"/>` },
  { name: 'pupils', markup: `<circle cx="26.8" cy="31" r="2.3" fill="${INK}"/><circle cx="38.8" cy="31" r="2.3" fill="${INK}"/>` },
  {
    name: 'lids',
    markup: `<path d="M21.4 30A4.6 4.6 0 0 1 30.6 30ZM33.4 30A4.6 4.6 0 0 1 42.6 30Z" fill="${PINK}"/><path d="M21.6 30H30.4M33.6 30H42.4" stroke="#C93F66" stroke-width="1.2" stroke-linecap="round"/>`,
  },
  { name: 'happy', markup: `<path d="M22.2 31.4Q26 26.6 29.8 31.4M34.2 31.4Q38 26.6 41.8 31.4" stroke="${INK}" stroke-width="2.6" stroke-linecap="round"/>` },
  {
    name: 'dizzy',
    markup: `<path d="M23.4 27.6l5.2 5.2M28.6 27.6l-5.2 5.2M35.4 27.6l5.2 5.2M40.6 27.6l-5.2 5.2" stroke="${INK}" stroke-width="2.4" stroke-linecap="round"/>`,
  },
  {
    name: 'cap',
    markup: `<path d="M24 17C25 8 39 8 40 17Z" transform="rotate(-14 32 15)" fill="${WHITE}"/><rect x="22.5" y="15" width="19" height="4.2" rx="2.1" transform="rotate(-14 32 15)" fill="${NAVY}"/>`,
  },
  {
    name: 'badge-needs',
    markup: `<circle cx="54" cy="11" r="8" fill="#F0D65A"/><rect x="53" y="5.5" width="2" height="6.5" rx="1" fill="#1A1608"/><circle cx="54" cy="15" r="1.2" fill="#1A1608"/>`,
  },
  {
    name: 'badge-done',
    markup: `<circle cx="54" cy="11" r="8" fill="#50D782"/><path d="M50.3 11.2l2.6 2.6 4.8-5.2" stroke="#0E0E13" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>`,
  },
  {
    name: 'badge-lost',
    markup: `<circle cx="54" cy="11" r="8" fill="#F04E5C"/><path d="M51 12.4a3.4 3.4 0 1 0 1.3-3.6" stroke="#FFFFFF" stroke-width="1.7" stroke-linecap="round"/><path d="M51.6 6.6l.8 2.4-2.4.9" stroke="#FFFFFF" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"/>`,
  },
];

export const SLEEPING_LAYERS = [
  { name: 'sleep-halo', markup: `<circle cx="100" cy="100" r="84" fill="#1F2B5C"/>` },
  {
    name: 'sleep-legs-outer',
    markup: `<path d="M70 110C58 128 50 140 54 156C57 166 68 164 66 156" ${SLEEP_LEG}/><path d="M130 110C142 128 150 140 146 156C143 166 132 164 134 156" ${SLEEP_LEG}/>`,
  },
  {
    name: 'sleep-legs-inner',
    markup: `<path d="M85 121C82 140 78 152 82 166C85 174 94 170 91 163" ${SLEEP_LEG}/><path d="M115 121C118 140 122 152 118 166C115 174 106 170 109 163" ${SLEEP_LEG}/>`,
  },
  { name: 'sleep-leg-center', markup: `<path d="M100 125C100 145 100 158 100 170" ${SLEEP_LEG}/>` },
  {
    name: 'sleep-head',
    markup: `<ellipse cx="100" cy="84" rx="46" ry="43" fill="${PINK}"/>
      <circle cx="124" cy="60" r="4.5" fill="${SPOT}"/><circle cx="135" cy="73" r="3" fill="${SPOT}"/><circle cx="69" cy="66" r="3" fill="${SPOT}"/>
      <ellipse cx="80" cy="60" rx="11" ry="6" transform="rotate(-25 80 60)" fill="#FFFFFF" fill-opacity="0.28"/>
      <circle cx="83" cy="90" r="10" fill="${WHITE}"/><circle cx="117" cy="90" r="10" fill="${WHITE}"/>
      <circle cx="84" cy="94" r="5" fill="${INK}"/><circle cx="118" cy="94" r="5" fill="${INK}"/>
      <path d="M73 90A10 10 0 0 1 93 90ZM107 90A10 10 0 0 1 127 90Z" fill="${PINK}"/>
      <path d="M73 90H93M107 90H127" stroke="#C93F66" stroke-width="2.2" stroke-linecap="round"/>
      <path d="M95 106H105" stroke="${INK}" stroke-width="3" stroke-linecap="round"/>
      <path d="M84 46C86 26 122 26 124 46Z" transform="rotate(-14 104 40)" fill="${WHITE}"/>
      <rect x="80" y="42" width="48" height="9" rx="4.5" transform="rotate(-14 104 40)" fill="${NAVY}"/>
      <circle cx="104" cy="25" r="5.5" transform="rotate(-14 104 40)" fill="${PINK}"/>`,
  },
  { name: 'z-1', markup: `<path d="M148 40H157L148 49H157" stroke="#9A9AA8" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"/>` },
  { name: 'z-2', markup: `<path d="M162 24H168L162 30H168" stroke="#9A9AA8" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"/>` },
];

export const MOODS = ['idle', 'working', 'needs', 'done', 'lost'];

function layer(layers, name, box) {
  const { markup } = layers.find((entry) => entry.name === name);
  return h(
    'span',
    { class: `kai-layer kai-${name}` },
    svg(`<svg viewBox="0 0 ${box} ${box}" width="100%" height="100%" fill="none" aria-hidden="true">${markup}</svg>`),
  );
}

export function glyph(mood = 'idle') {
  const part = (name) => layer(LAYERS, name, 64);
  return h(
    'span',
    { class: 'kai', dataset: { mood }, 'aria-hidden': 'true' },
    h(
      'span',
      { class: 'kai-body' },
      part('leg-1'),
      part('leg-2'),
      part('leg-3'),
      part('head'),
      h('span', { class: 'kai-face' }, part('eyes'), part('pupils'), part('lids'), part('happy'), part('dizzy')),
      part('cap'),
    ),
    part('badge-needs'),
    part('badge-done'),
    part('badge-lost'),
  );
}

export function sleeping() {
  const part = (name) => layer(SLEEPING_LAYERS, name, 200);
  return h(
    'span',
    { class: 'kai kai-sleeping', 'aria-hidden': 'true' },
    part('sleep-halo'),
    h('span', { class: 'kai-body' }, part('sleep-legs-outer'), part('sleep-legs-inner'), part('sleep-leg-center'), part('sleep-head')),
    part('z-1'),
    part('z-2'),
  );
}

export function moodOf(agents) {
  const live = agents.filter((agent) => !agent.starting);
  if (live.some((agent) => agent.state === 'paused')) return 'needs';
  if (agents.some((agent) => agent.state === 'working' || agent.starting)) return 'working';
  if (live.some((agent) => agent.state === 'done')) return 'done';
  if (live.some((agent) => agent.state === 'lost')) return 'lost';
  return 'idle';
}
