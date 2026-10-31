import { $, h } from './dom.js';

const LIMIT = 4;

export function toast(text, tone = 'info') {
  const host = $('toasts');
  const node = h('div', { class: 'toast', dataset: { tone } }, text);
  host.append(node);
  while (host.children.length > LIMIT) host.firstElementChild.remove();
  setTimeout(() => node.remove(), tone === 'error' ? 7000 : 4000);
}
