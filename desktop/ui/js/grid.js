import { $, h, keep, place, svg } from './dom.js';
import { tools } from './board.js';
import { activity, alive, elapsed, glyph, gridShape, kindColor, label, nameTone, previewRows } from './format.js';
import { ICON_EXPAND, ICON_PLUS } from './icons.js';
import { now, state, visible } from './state.js';
import { Terminal, run } from './terminal.js';
import { toast } from './toast.js';

export const MAX_PEEK_AGENTS = 24;
const LINE_HEIGHT = 15;
const LIVE_FONT_SIZE = 12;

const tiles = new Map();
const shownPreviews = new Map();
const live = { agent: null, host: null, terminal: null };
let addTile = null;

export function rowsOnScreen() {
  const screen = $('grid').querySelector('.tile-screen');
  return previewRows(screen ? screen.clientHeight : 0, LINE_HEIGHT);
}

export function peekRequest(list, rows) {
  const running = list.filter((agent) => alive(agent) && !agent.starting && agent.id !== live.agent);
  if (!running.length) return null;
  return { type: 'peek', agents: running.slice(0, MAX_PEEK_AGENTS).map((agent) => agent.id), rows };
}

function openInTab(agent, handlers) {
  return h(
    'button',
    {
      class: 'icon-button',
      type: 'button',
      title: 'Open in a tab',
      onclick: (event) => {
        event.stopPropagation();
        handlers.open(agent);
      },
    },
    svg(ICON_EXPAND),
  );
}

function tile(agent, handlers) {
  const mark = glyph(agent, state.frame);
  const time = elapsed(agent, now());
  const waiting = agent.state === 'paused' && !agent.starting;
  const classes = ['tile', waiting ? 'waiting' : '', agent.state === 'lost' ? 'lost' : ''].filter(Boolean).join(' ');
  const toolbar = tools(agent);
  toolbar.prepend(openInTab(agent, handlers));
  return h(
    'article',
    { class: classes, tabindex: '0', title: `${agent.name} · ${agent.project}`, dataset: { agent: String(agent.id) }, onclick: () => activate(agent, handlers) },
    toolbar,
    h(
      'div',
      { class: 'tile-head' },
      h('span', { class: `glyph ${mark.tone}`, dataset: mark.spin ? { spin: '1' } : {} }, mark.text),
      h('span', { class: `tile-name ${waiting ? 't-yellow' : nameTone(agent)}` }, agent.name),
      h('span', { class: 'tile-kind', style: `color:${kindColor(agent.kind)}` }, label(agent.kind)),
      h('span', { class: `tile-time ${time ? time.tone : ''}` }, time ? time.text : ''),
    ),
    h('div', { class: 'tile-screen' }),
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
  node.classList.remove('live');
  const next = screen(agent);
  if (shownPreviews.get(agent.id) === next.signature && node.querySelector('.tile-screen').childElementCount) return;
  shownPreviews.set(agent.id, next.signature);
  node.querySelector('.tile-screen').replaceChildren(...next.nodes);
}

function showLive(node) {
  node.classList.add('live');
  const target = node.querySelector('.tile-screen');
  if (live.host.parentElement !== target) target.replaceChildren(live.host);
}

function activate(agent, handlers) {
  if (live.agent === agent.id) {
    live.terminal.focus();
    return;
  }
  deactivateGrid();
  const cached = tiles.get(agent.id);
  if (!cached) return;
  live.agent = agent.id;
  live.host = live.host || h('div', { class: 'grid-term' });
  shownPreviews.delete(agent.id);
  showLive(cached.node);
  live.terminal = live.terminal || new Terminal(live.host, handlers.term, { fontSize: LIVE_FONT_SIZE });
  live.terminal.open(agent.id).catch((err) => toast(`could not open the terminal: ${err}`, 'error'));
  live.terminal.focus();
}

export function deactivateGrid() {
  if (live.agent === null) return;
  const cached = tiles.get(live.agent);
  shownPreviews.delete(live.agent);
  live.agent = null;
  live.terminal.close();
  live.host.remove();
  if (cached) cached.node.classList.remove('live');
}

function tileSignature(agent, handlers) {
  return JSON.stringify([agent.name, agent.kind, agent.state, agent.starting, agent.project, Boolean(handlers.open)]);
}

function newAgentTile() {
  addTile = addTile || h('button', { class: 'tile-new', type: 'button', onclick: () => $('task').focus() }, svg(ICON_PLUS), 'Write a task below to add an agent');
  return addTile;
}

function shape(grid) {
  const fixed = gridShape(state.gridSize);
  grid.classList.toggle('all', !fixed);
  grid.style.setProperty('--grid-cols', fixed ? String(fixed.cols) : '');
  grid.style.setProperty('--grid-rows', fixed ? String(fixed.rows) : '');
}

export function renderGrid(handlers) {
  const grid = $('grid');
  shape(grid);
  const list = visible();
  if (live.agent !== null && !list.some((agent) => agent.id === live.agent)) deactivateGrid();
  const focused = Boolean(live.host && live.host.contains(document.activeElement));
  const nodes = list.map((agent) => {
    const node = keep(tiles, agent.id, tileSignature(agent, handlers), () => tile(agent, handlers));
    if (agent.id === live.agent) showLive(node);
    else showScreen(node, agent);
    return node;
  });
  place(grid, nodes.length ? nodes : [newAgentTile()]);
  if (focused && live.agent !== null) live.terminal.focus();
  const current = list.find((agent) => agent.id === live.agent);
  if (current && alive(current) && !current.starting && live.terminal.stale(current.id)) {
    live.terminal.open(current.id).catch((err) => toast(`could not open the terminal: ${err}`, 'error'));
  }
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
