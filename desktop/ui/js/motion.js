import { $ } from './dom.js';

const TIP_DELAY_MS = 450;
const TIP_WARM_MS = 500;
const TIP_GAP = 8;
const TIP_MARGIN = 8;

let tipTarget = null;
let tipTimer = null;
let tipHiddenAt = 0;

export function reducedMotion() {
  return window.matchMedia('(prefers-reduced-motion: reduce)').matches;
}

function token(name) {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim();
}

export function motionMs(name) {
  return reducedMotion() ? 0 : parseFloat(token(name)) || 0;
}

export function motion(duration) {
  return { duration: motionMs(duration), easing: token('--ease-out') || 'ease-out' };
}

export function settle(act) {
  setTimeout(act, motionMs('--motion-slow'));
}

export function flash(node, name) {
  node.classList.remove(name);
  void node.offsetWidth;
  node.classList.add(name);
  node.addEventListener('animationend', () => node.classList.remove(name), { once: true });
}

export function slideThumb(group) {
  const active = group.querySelector('button[aria-pressed="true"]');
  if (!active || !active.offsetWidth) return;
  group.style.setProperty('--thumb-x', `${active.offsetLeft}px`);
  group.style.setProperty('--thumb-width', `${active.offsetWidth}px`);
  group.classList.add('sliding');
}

export function choose(button, act) {
  const group = button.closest('.segmented');
  for (const other of group.querySelectorAll('button')) other.setAttribute('aria-pressed', String(other === button));
  slideThumb(group);
  settle(act);
}

export function rise(node, options) {
  node.animate([{ opacity: 0, transform: 'translateY(10px) scale(0.97)' }, { opacity: 1, transform: 'none' }], options);
}

export function glide(node, from, options) {
  const to = node.getBoundingClientRect();
  const dx = from.left - to.left;
  const dy = from.top - to.top;
  if (Math.abs(dx) < 1 && Math.abs(dy) < 1) return;
  node.animate([{ transform: `translate(${dx}px, ${dy}px)` }, { transform: 'none' }], options);
}

function tipText(node) {
  const title = node.getAttribute('title');
  if (title !== null) {
    node.dataset.tip = title;
    node.removeAttribute('title');
  }
  return node.dataset.tip || '';
}

function placeTip(tip, target) {
  const layer = target.closest('dialog[open]') || document.body;
  if (tip.parentElement !== layer) layer.append(tip);
  const box = target.getBoundingClientRect();
  const below = box.bottom + TIP_GAP + tip.offsetHeight <= window.innerHeight - TIP_MARGIN;
  const centered = box.left + box.width / 2 - tip.offsetWidth / 2;
  const left = Math.min(Math.max(TIP_MARGIN, centered), window.innerWidth - tip.offsetWidth - TIP_MARGIN);
  tip.dataset.side = below ? 'below' : 'above';
  tip.style.left = `${left}px`;
  tip.style.top = `${below ? box.bottom + TIP_GAP : box.top - TIP_GAP - tip.offsetHeight}px`;
}

function showTip() {
  tipTimer = null;
  const text = tipTarget && tipTarget.isConnected ? tipText(tipTarget) : '';
  if (!text) {
    hideTip();
    return;
  }
  const tip = $('tip');
  tip.textContent = text;
  placeTip(tip, tipTarget);
  tip.classList.add('shown');
}

function hideTip() {
  clearTimeout(tipTimer);
  tipTimer = null;
  tipTarget = null;
  const tip = $('tip');
  if (tip.classList.contains('shown')) {
    tip.classList.remove('shown');
    tipHiddenAt = Date.now();
  }
}

function onOver(event) {
  const target = event.target instanceof Element ? event.target.closest('[title], [data-tip]') : null;
  if (target === tipTarget) return;
  if (!target) {
    hideTip();
    return;
  }
  const warm = $('tip').classList.contains('shown') || Date.now() - tipHiddenAt < TIP_WARM_MS;
  hideTip();
  tipTarget = target;
  tipText(target);
  if (warm) showTip();
  else tipTimer = setTimeout(showTip, TIP_DELAY_MS);
}

function onMove() {
  if (!tipTarget || !tipTarget.hasAttribute('title')) return;
  const text = tipText(tipTarget);
  const tip = $('tip');
  if (tip.classList.contains('shown')) tip.textContent = text;
}

export function initTooltips() {
  document.addEventListener('pointerover', onOver);
  document.addEventListener('pointermove', onMove, { passive: true });
  document.addEventListener('pointerout', (event) => {
    if (!event.relatedTarget) hideTip();
  });
  for (const type of ['pointerdown', 'keydown', 'wheel']) document.addEventListener(type, hideTip, { capture: true, passive: true });
  window.addEventListener('blur', hideTip);
}
