import { $, h, keep, place, svg } from './dom.js';
import { confirm } from './dialogs.js';
import { COLUMNS, SPINNER, activity, alive, column, elapsed, glyph, kindColor, label, nameTone, plural } from './format.js';
import { ICON_BRANCH, ICON_STOP, ICON_TRASH } from './icons.js';
import { sleeping } from './kai.js';
import { inSession, now, state } from './state.js';
import { send } from './transport.js';

async function stopAgent(agent) {
  const sure = await confirm({
    title: `Stop ${agent.name}?`,
    text: 'The agent process ends. Its card stays here until you remove it.',
    action: 'Stop',
    danger: true,
  });
  if (sure) send({ type: 'stop', agent: agent.id });
}

async function removeAgent(agent) {
  const worktree = agent.branch ? ` Its worktree and the ${agent.branch} branch are deleted too.` : '';
  const sure = await confirm({
    title: `Remove ${agent.name}?`,
    text: `The agent and its recorded output are deleted.${worktree}`,
    action: 'Remove',
    danger: true,
  });
  if (sure) send({ type: 'remove', agent: agent.id });
}

function tools(agent) {
  const act = (work) => (event) => {
    event.stopPropagation();
    work(agent);
  };
  const button = alive(agent)
    ? h('button', { class: 'icon-button danger', type: 'button', title: 'Stop', onclick: act(stopAgent) }, svg(ICON_STOP))
    : h('button', { class: 'icon-button danger', type: 'button', title: 'Remove', onclick: act(removeAgent) }, svg(ICON_TRASH));
  return h('div', { class: 'card-tools' }, button);
}

function card(agent, handlers) {
  const mark = glyph(agent, state.frame);
  const time = elapsed(agent, now());
  const doing = activity(agent);
  const waiting = agent.state === 'paused' && !agent.starting;
  const classes = ['card', waiting ? 'waiting' : '', agent.state === 'lost' ? 'lost' : ''].filter(Boolean).join(' ');
  const text = waiting
    ? h('div', { class: 'card-text' }, h('span', { class: 'card-strong' }, 'Waiting for your answer'), h('span', { class: 't-muted' }, agent.activity))
    : h('div', { class: `card-text ${doing.tone}${agent.kind === 'terminal' ? ' mono' : ''}` }, doing.text);
  const open = handlers.open
    ? h(
        'div',
        { class: 'card-actions' },
        h('button', { class: 'button waiting', type: 'button', onclick: () => handlers.open(agent) }, 'Open agent'),
        h('span', { class: 'kbd' }, 'alt N'),
      )
    : null;
  const branch = agent.branch
    ? h('div', { class: 'card-foot' }, h('span', { class: 'branch' }, svg(ICON_BRANCH), agent.branch))
    : null;
  return h(
    'article',
    { class: classes, dataset: { agent: String(agent.id) }, onclick: handlers.open ? () => handlers.open(agent) : null },
    tools(agent),
    h(
      'div',
      { class: 'card-top' },
      h('span', { class: `glyph ${mark.tone}`, dataset: mark.spin ? { spin: '1' } : {} }, mark.text),
      h('span', { class: `card-name ${nameTone(agent)}` }, agent.name),
      time ? h('span', { class: `card-time ${time.tone}` }, time.text) : null,
    ),
    h(
      'div',
      { class: 'card-meta' },
      h('span', { class: 'swatch', style: `background:${agent.color}` }),
      h('span', { class: 'project' }, agent.project),
      h('span', { class: 't-faint' }, '·'),
      h('span', { style: `color:${kindColor(agent.kind)}` }, label(agent.kind)),
    ),
    text,
    waiting ? open : null,
    branch,
  );
}

function columnHead(def, count) {
  const mark = {
    needs: h('span', { class: 'dot' }),
    working: h('span', { class: 'glyph t-text', dataset: { spin: '1' } }, SPINNER[state.frame % SPINNER.length]),
    done: h('span', { class: 'glyph t-green' }, '✓'),
    idle: h('span', { class: 'glyph t-faint' }, '●'),
  }[def.key];
  return h('div', { class: 'column-head' }, mark, h('span', { class: 'name' }, def.title), h('span', { class: 'count' }, String(count)));
}

function empty(session) {
  return h(
    'div',
    { class: 'empty' },
    sleeping(),
    h('span', { class: 'empty-title' }, `No agents in ${session} yet`),
    h('span', null, 'Write a task below and press enter: the agent starts already working on it.'),
  );
}

const cards = new Map();
const columns = new Map();

function cardSignature(agent, handlers) {
  return JSON.stringify([agent.name, agent.kind, agent.state, agent.starting, agent.activity, agent.project, agent.color, agent.branch, agent.exit, Boolean(handlers.open)]);
}

export function renderBoard(handlers) {
  const board = $('board');
  const list = inSession();
  if (!list.length) {
    const signature = `empty:${state.session}`;
    if (board.dataset.signature !== signature) {
      board.replaceChildren(empty(state.session));
      board.dataset.signature = signature;
    }
    cards.clear();
    return;
  }
  const sections = COLUMNS.map((def) => {
    const members = list.filter((agent) => column(agent) === def.key);
    const nodes = members.length
      ? members.map((agent) => keep(cards, agent.id, cardSignature(agent, handlers), () => card(agent, handlers)))
      : [keep(columns, `empty:${def.key}`, '', () => h('div', { class: 'column-empty' }, def.empty))];
    const section = keep(columns, def.key, '', () =>
      h('section', { class: 'column', dataset: { column: def.key } }, columnHead(def, 0), h('div', { class: 'column-body' })),
    );
    section.querySelector('.count').textContent = String(members.length);
    place(section.querySelector('.column-body'), nodes);
    return section;
  });
  place(board, sections);
  board.dataset.signature = 'columns';
  const ids = new Set(list.map((agent) => agent.id));
  for (const id of cards.keys()) if (!ids.has(id)) cards.delete(id);
}

export function renderHead() {
  const list = inSession();
  const projects = new Set(list.map((agent) => agent.project)).size;
  const waiting = list.filter((agent) => agent.state === 'paused' && !agent.starting).length;
  const lost = list.filter((agent) => agent.state === 'lost').length;
  $('session-title').textContent = state.session || '';
  const summary = $('session-summary');
  const parts = [list.length ? `${plural(list.length, 'agent', 'agents')} across ${plural(projects, 'project', 'projects')}` : 'No agents yet'];
  if (waiting) parts.push(h('span', { class: 't-yellow' }, ` · ${waiting} need${waiting === 1 ? 's' : ''} you`));
  summary.replaceChildren(...parts);
  const recover = $('recover');
  recover.hidden = lost === 0;
  recover.textContent = `Recover ${plural(lost, 'interrupted agent', 'interrupted agents')}`;
}

export function tickBoard() {
  const byId = new Map(inSession().map((agent) => [String(agent.id), agent]));
  for (const node of $('board').querySelectorAll('[data-agent]')) {
    const agent = byId.get(node.dataset.agent);
    const target = node.querySelector('.card-time');
    if (!agent || !target) continue;
    const time = elapsed(agent, now());
    if (time) target.textContent = time.text;
  }
}

export function spinBoard() {
  for (const node of $('board').querySelectorAll('[data-spin]')) node.textContent = SPINNER[state.frame % SPINNER.length];
}
