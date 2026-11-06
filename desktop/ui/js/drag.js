import { h } from './dom.js';
import { canMove } from './format.js';
import { findAgent } from './state.js';
import { send } from './transport.js';

const HANDLES = '#board .card, #list-body .row, #grid .tile-head';
const NOT_HANDLES = 'button, input, textarea, a, .xterm';
const START_AFTER_PX = 6;
const GHOST_OFFSET_PX = 12;
const CLICK_GUARD_MS = 50;

let press = null;
let moving = null;

function swallow(event) {
  event.stopPropagation();
  event.preventDefault();
}

function sessionAt(x, y) {
  const row = document.elementFromPoint(x, y)?.closest('.session-row[data-session]');
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

function drop(event) {
  const session = sessionAt(event.clientX, event.clientY)?.dataset.session;
  if (session) send({ type: 'moveAgent', agent: moving.agent.id, session });
  finish();
  window.addEventListener('click', swallow, { capture: true, once: true });
  setTimeout(() => window.removeEventListener('click', swallow, { capture: true }), CLICK_GUARD_MS);
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

export function initAgentDrag() {
  document.addEventListener('pointerdown', onPointerDown);
  document.addEventListener('pointermove', onPointerMove);
  document.addEventListener('pointerup', onPointerUp);
  document.addEventListener('pointercancel', callOff);
  window.addEventListener('blur', callOff);
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
