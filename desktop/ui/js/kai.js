import { svg } from './dom.js';

const PINK = '#FF5F87';

const GLYPH = `<svg class="kai" data-mood="idle" viewBox="0 0 64 64" width="100%" height="100%" fill="none" aria-hidden="true">
  <g class="kai-body">
    <g class="kai-legs" stroke="${PINK}" stroke-width="6.5" stroke-linecap="round">
      <path class="leg leg-1" d="M22 38C18 45 14 47 12 53"/>
      <path class="leg leg-2" d="M32 41V55"/>
      <path class="leg leg-3" d="M42 38C46 45 50 47 52 53"/>
    </g>
    <ellipse cx="32" cy="29" rx="17" ry="16" fill="${PINK}"/>
    <g class="kai-eyes">
      <circle cx="26" cy="30" r="4.4" fill="#ECECF2"/><circle cx="38" cy="30" r="4.4" fill="#ECECF2"/>
      <g class="kai-pupils"><circle cx="26.8" cy="31" r="2.3" fill="#121218"/><circle cx="38.8" cy="31" r="2.3" fill="#121218"/></g>
    </g>
    <g class="kai-cap">
      <path d="M24 17C25 8 39 8 40 17Z" transform="rotate(-14 32 15)" fill="#ECECF2"/>
      <rect x="22.5" y="15" width="19" height="4.2" rx="2.1" transform="rotate(-14 32 15)" fill="#2C3C78"/>
    </g>
  </g>
  <g class="kai-badge kai-badge-needs"><circle cx="54" cy="11" r="8" fill="#F0D65A"/><rect x="53" y="5.5" width="2" height="6.5" rx="1" fill="#1A1608"/><circle cx="54" cy="15" r="1.2" fill="#1A1608"/></g>
  <g class="kai-badge kai-badge-done"><circle cx="54" cy="11" r="8" fill="#50D782"/><path d="M50.3 11.2l2.6 2.6 4.8-5.2" stroke="#0E0E13" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/></g>
  <g class="kai-badge kai-badge-lost"><circle cx="54" cy="11" r="8" fill="#F04E5C"/><path d="M51 12.4a3.4 3.4 0 1 0 1.3-3.6" stroke="#FFFFFF" stroke-width="1.7" stroke-linecap="round"/><path d="M51.6 6.6l.8 2.4-2.4.9" stroke="#FFFFFF" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"/></g>
</svg>`;

const SLEEPING = `<svg class="kai kai-sleeping" viewBox="0 0 200 200" width="180" height="180" fill="none" aria-hidden="true">
  <circle cx="100" cy="100" r="84" fill="#1F2B5C"/>
  <g class="kai-body">
    <g stroke="${PINK}" stroke-width="13" stroke-linecap="round">
      <path class="leg leg-1" d="M70 110C58 128 50 140 54 156C57 166 68 164 66 156"/>
      <path class="leg leg-2" d="M85 121C82 140 78 152 82 166C85 174 94 170 91 163"/>
      <path class="leg leg-3" d="M100 125C100 145 100 158 100 170"/>
      <path class="leg leg-2" d="M115 121C118 140 122 152 118 166C115 174 106 170 109 163"/>
      <path class="leg leg-1" d="M130 110C142 128 150 140 146 156C143 166 132 164 134 156"/>
    </g>
    <ellipse cx="100" cy="84" rx="46" ry="43" fill="${PINK}"/>
    <circle cx="124" cy="60" r="4.5" fill="#E2486F"/><circle cx="135" cy="73" r="3" fill="#E2486F"/><circle cx="69" cy="66" r="3" fill="#E2486F"/>
    <ellipse cx="80" cy="60" rx="11" ry="6" transform="rotate(-25 80 60)" fill="#FFFFFF" fill-opacity="0.28"/>
    <circle cx="83" cy="90" r="10" fill="#ECECF2"/><circle cx="117" cy="90" r="10" fill="#ECECF2"/>
    <circle cx="84" cy="94" r="5" fill="#121218"/><circle cx="118" cy="94" r="5" fill="#121218"/>
    <path d="M73 90A10 10 0 0 1 93 90ZM107 90A10 10 0 0 1 127 90Z" fill="${PINK}"/>
    <path d="M73 90H93M107 90H127" stroke="#C93F66" stroke-width="2.2" stroke-linecap="round"/>
    <path d="M95 106H105" stroke="#121218" stroke-width="3" stroke-linecap="round"/>
    <path d="M84 46C86 26 122 26 124 46Z" transform="rotate(-14 104 40)" fill="#ECECF2"/>
    <rect x="80" y="42" width="48" height="9" rx="4.5" transform="rotate(-14 104 40)" fill="#2C3C78"/>
    <circle cx="104" cy="25" r="5.5" transform="rotate(-14 104 40)" fill="${PINK}"/>
  </g>
  <g stroke="#9A9AA8" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
    <path class="kai-z kai-z-1" d="M148 40H157L148 49H157"/>
    <path class="kai-z kai-z-2" d="M162 24H168L162 30H168"/>
  </g>
</svg>`;

export const MOODS = ['idle', 'working', 'needs', 'done', 'lost'];

export function glyph(mood = 'idle') {
  const node = svg(GLYPH);
  node.dataset.mood = mood;
  return node;
}

export function sleeping() {
  return svg(SLEEPING);
}

export function moodOf(agents) {
  const live = agents.filter((agent) => !agent.starting);
  if (live.some((agent) => agent.state === 'paused')) return 'needs';
  if (agents.some((agent) => agent.state === 'working' || agent.starting)) return 'working';
  if (live.some((agent) => agent.state === 'done')) return 'done';
  if (live.some((agent) => agent.state === 'lost')) return 'lost';
  return 'idle';
}
