import { $, h } from './dom.js';
import { canMove, moveTab } from './format.js';
import { glide, motion } from './motion.js';
import { findAgent, saveTabs, state } from './state.js';
import { send } from './transport.js';

const HANDLES = '#board .card, #list-body .row, #grid .tile-head, #sessions .summary';
const NOT_HANDLES = 'button, input, textarea, a, .xterm';
const TAB_CONTROLS = '.tab-close, .tab-pin';
const START_AFTER_PX = 6;
const GHOST_OFFSET_PX = 12;
const CLICK_GUARD_MS = 50;

let press = null;
let moving = null;
let tabPress = null;
let tabMoving = null;

function swallow(event) {
  event.stopPropagation();
  event.preventDefault();
}

function sessionAt(x, y) {
  const row = document.elementFromPoint(x, y)?.closest('.session')?.querySelector('.session-row[data-session]');
  return row && canMove(moving.agent, row.dataset.session) ? row : null;
}

function mark(row) {
  if (moving.target === row) return;
  moving.target?.classList.remove('drop-target');
  row?.classList.add('drop-target');
  moving.target = row;
}

function follow(event) {
  moving.ghost.style.transform = `translate(${event.clientX + GHOST_OFFSET_PX}px, ${event.clientY + GHOST_OFFSET_PX}px)`;
  mark(sessionAt(event.clientX, event.clientY));
}

function begin(event) {
  const agent = findAgent(press.agent);
  const pointer = press.pointer;
  const source = press.source;
  press = null;
  if (!agent || agent.starting) return;
  const ghost = h('div', { class: 'drag-ghost' }, h('span', { class: 'swatch', style: `background:${agent.color}` }), h('span', { class: 'drag-ghost-name' }, agent.name));
  moving = { agent, ghost, source, target: null };
  source.classList.add('moving-source');
  document.body.append(ghost);
  document.documentElement.classList.add('moving-agent');
  document.documentElement.addEventListener('lostpointercapture', callOff, { once: true });
  document.documentElement.setPointerCapture(pointer);
  follow(event);
}

function finish() {
  const { ghost, source, target } = moving;
  ghost.remove();
  source.classList.remove('moving-source');
  target?.classList.remove('drop-target');
  document.documentElement.classList.remove('moving-agent');
  moving = null;
}

function guardClick() {
  window.addEventListener('click', swallow, { capture: true, once: true });
  setTimeout(() => window.removeEventListener('click', swallow, { capture: true }), CLICK_GUARD_MS);
}

function drop(event) {
  const session = sessionAt(event.clientX, event.clientY)?.dataset.session;
  if (session) send({ type: 'moveAgent', agent: moving.agent.id, session });
  finish();
  guardClick();
}

function onPointerDown(event) {
  if (event.button !== 0 || moving || !(event.target instanceof Element) || event.target.closest(NOT_HANDLES)) return;
  const source = event.target.closest(HANDLES)?.closest('[data-agent]');
  if (source) press = { agent: Number(source.dataset.agent), source, pointer: event.pointerId, x: event.clientX, y: event.clientY };
}

function onPointerMove(event) {
  if (moving) follow(event);
  else if (press && Math.hypot(event.clientX - press.x, event.clientY - press.y) >= START_AFTER_PX) begin(event);
}

function onPointerUp(event) {
  press = null;
  if (moving) drop(event);
}

function callOff() {
  press = null;
  if (moving) finish();
}

function tabNodes() {
  return [...$('tabs').querySelectorAll('.tab')];
}

function slideTab(left) {
  const node = $('tabs').querySelector('.tab.moving');
  if (!node) return;
  const group = tabNodes().filter((tab) => tab.classList.contains('pinned') === node.classList.contains('pinned'));
  const last = group[group.length - 1];
  const at = Math.min(Math.max(left, group[0].offsetLeft), last.offsetLeft + last.offsetWidth - node.offsetWidth);
  node.style.transform = `translateX(${at - node.offsetLeft}px)`;
}

function followTab(event) {
  const x = event.clientX - $('tabs').getBoundingClientRect().left;
  const others = tabNodes().filter((node) => node.dataset.tab !== String(tabMoving.id));
  const index = others.filter((node) => node.offsetLeft + node.offsetWidth / 2 < x).length;
  const next = moveTab(state.tabs, state.pinned, tabMoving.id, index);
  if (next !== state.tabs) {
    const places = new Map(others.map((node) => [node.dataset.tab, node.getBoundingClientRect()]));
    state.tabs = next;
    tabMoving.render();
    const options = motion('--motion-fast');
    for (const node of tabNodes()) if (places.has(node.dataset.tab)) glide(node, places.get(node.dataset.tab), options);
  }
  slideTab(x - tabMoving.grab);
}

function beginTabMove(event, render) {
  const { id, grab, pointer } = tabPress;
  tabPress = null;
  tabMoving = { id, grab, render, before: state.tabs };
  state.movingTab = id;
  render();
  document.documentElement.classList.add('moving-tab');
  document.documentElement.addEventListener('lostpointercapture', callOffTabMove, { once: true });
  document.documentElement.setPointerCapture(pointer);
  followTab(event);
}

function endTabMove() {
  const { render } = tabMoving;
  tabMoving = null;
  state.movingTab = null;
  document.documentElement.classList.remove('moving-tab');
  render();
}

function callOffTabMove() {
  tabPress = null;
  if (!tabMoving) return;
  state.tabs = tabMoving.before;
  endTabMove();
}

function onTabPointerDown(event) {
  if (event.button !== 0 || tabMoving || !(event.target instanceof Element) || event.target.closest(TAB_CONTROLS)) return;
  const node = event.target.closest('#tabs .tab');
  if (!node) return;
  const id = state.tabs.find((tab) => String(tab) === node.dataset.tab);
  const grab = event.clientX - $('tabs').getBoundingClientRect().left - node.offsetLeft;
  tabPress = { id, grab, pointer: event.pointerId, x: event.clientX, y: event.clientY };
}

function onTabPointerUp() {
  tabPress = null;
  if (!tabMoving) return;
  saveTabs();
  endTabMove();
  guardClick();
}

export function initTabDrag(render) {
  document.addEventListener('pointerdown', onTabPointerDown);
  document.addEventListener('pointermove', (event) => {
    if (tabMoving) followTab(event);
    else if (tabPress && Math.hypot(event.clientX - tabPress.x, event.clientY - tabPress.y) >= START_AFTER_PX) beginTabMove(event, render);
  });
  document.addEventListener('pointerup', onTabPointerUp);
  document.addEventListener('pointercancel', callOffTabMove);
  document.addEventListener(
    'keydown',
    (event) => {
      if (event.key === 'Escape' && tabMoving) {
        swallow(event);
        callOffTabMove();
      }
    },
    { capture: true },
  );
}

export function initAgentDrag() {
  document.addEventListener('pointerdown', onPointerDown);
  document.addEventListener('pointermove', onPointerMove);
  document.addEventListener('pointerup', onPointerUp);
  document.addEventListener('pointercancel', callOff);
  document.addEventListener(
    'keydown',
    (event) => {
      if (event.key === 'Escape' && moving) {
        swallow(event);
        callOff();
      }
    },
    { capture: true },
  );
}
