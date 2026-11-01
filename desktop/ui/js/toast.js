import { $, h } from './dom.js';
import { motionMs } from './motion.js';

const LIMIT = 4;

function dismiss(node) {
  node.classList.add('leaving');
  setTimeout(() => node.remove(), motionMs('--motion-base'));
}

export function toast(text, tone = 'info') {
  const host = $('toasts');
  const node = h('div', { class: 'toast', dataset: { tone } }, text);
  host.append(node);
  while (host.children.length > LIMIT) host.firstElementChild.remove();
  setTimeout(() => dismiss(node), tone === 'error' ? 7000 : 4000);
}
