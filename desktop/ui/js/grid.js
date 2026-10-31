import { $, h, keep, place, svg } from './dom.js';
import { tools } from './board.js';
import { activity, alive, elapsed, glyph, kindColor, label, nameTone } from './format.js';
import { ICON_PLUS } from './icons.js';
import { now, state, visible } from './state.js';
import { run } from './terminal.js';

export const MAX_PEEK_AGENTS = 24;
export const PREVIEW_ROWS = 7;

const tiles = new Map();
const shownPreviews = new Map();
let addTile = null;

export function peekRequest(list) {
  const running = list.filter((agent) => alive(agent) && !agent.starting);
  if (!running.length) return null;
  return { type: 'peek', agents: running.slice(0, MAX_PEEK_AGENTS).map((agent) => agent.id), rows: PREVIEW_ROWS };
}

function tile(agent, handlers) {
  const mark = glyph(agent, state.frame);
  const time = elapsed(agent, now());
  const waiting = agent.state === 'paused' && !agent.starting;
  const classes = ['tile', waiting ? 'waiting' : '', agent.state === 'lost' ? 'lost' : ''].filter(Boolean).join(' ');
  return h(
    'article',
    { class: classes, tabindex: '0', dataset: { agent: String(agent.id) }, onclick: () => handlers.open(agent) },
    tools(agent),
    h(
      'div',
      { class: 'tile-head' },
      h('span', { class: `glyph ${mark.tone}`, dataset: mark.spin ? { spin: '1' } : {} }, mark.text),
      h('span', { class: `tile-name ${waiting ? 't-yellow' : nameTone(agent)}`, title: agent.name }, agent.name),
      h('span', { class: `tile-time ${time ? time.tone : ''}` }, time ? time.text : ''),
    ),
    h('div', { class: 'tile-screen' }),
    h(
      'div',
      { class: 'tile-foot' },
      h('span', { class: 'swatch', style: `background:${agent.color}` }),
      h('span', { class: 'project' }, agent.project),
      h('span', { class: 't-faint' }, '·'),
      h('span', { style: `color:${kindColor(agent.kind)}` }, label(agent.kind)),
    ),
  );
}

function screen(agent) {
  const lines = alive(agent) ? state.previews.get(agent.id) : null;
  if (!lines) {
    const text = activity(agent);
    return { signature: `text:${text.text}`, nodes: [h('div', { class: `tile-fallback ${text.tone}` }, text.text)] };
  }
  return {
    signature: JSON.stringify(lines),
    nodes: lines.map((runs) => h('div', { class: 'tile-line' }, ...runs.map(run))),
  };
}

function showScreen(node, agent) {
  const next = screen(agent);
  if (shownPreviews.get(agent.id) === next.signature && node.querySelector('.tile-screen').childElementCount) return;
  shownPreviews.set(agent.id, next.signature);
  node.querySelector('.tile-screen').replaceChildren(...next.nodes);
}

function tileSignature(agent, handlers) {
  return JSON.stringify([agent.name, agent.kind, agent.state, agent.starting, agent.project, agent.color, Boolean(handlers.open)]);
}

function newAgentTile() {
  addTile = addTile || h('button', { class: 'tile-new', type: 'button', onclick: () => $('task').focus() }, svg(ICON_PLUS), 'Write a task below to add an agent');
  return addTile;
}

export function renderGrid(handlers) {
  const list = visible();
  const nodes = list.map((agent) => {
    const node = keep(tiles, agent.id, tileSignature(agent, handlers), () => tile(agent, handlers));
    showScreen(node, agent);
    return node;
  });
  place($('grid'), [...nodes, newAgentTile()]);
  const ids = new Set(list.map((agent) => agent.id));
  for (const id of tiles.keys()) {
    if (ids.has(id)) continue;
    tiles.delete(id);
    shownPreviews.delete(id);
  }
}

export function tickGrid() {
  const byId = new Map(visible().map((agent) => [String(agent.id), agent]));
  for (const node of $('grid').querySelectorAll('.tile[data-agent]')) {
    const agent = byId.get(node.dataset.agent);
    if (!agent) continue;
    const time = elapsed(agent, now());
    node.querySelector('.tile-time').textContent = time ? time.text : '';
  }
}
