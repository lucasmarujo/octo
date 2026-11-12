import { h } from './dom.js';

const OLD_THEME = 'old';

export function usesKai() {
  return document.documentElement.dataset.theme === OLD_THEME;
}

export function mark(extra) {
  return h('span', { class: `octo-mark${extra ? ` ${extra}` : ''}`, 'aria-hidden': 'true' });
}

export function spinner() {
  return mark('octo-spin');
}
