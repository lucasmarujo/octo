import { $, h, keep, place, svg } from './dom.js';
import { tools } from './board.js';
import { activity, alive, changeParts, duration, elapsed, glyph, groupByProject, kindColor, label, nameTone, subagentLine, testMark } from './format.js';
import { ICON_BRANCH } from './icons.js';
import { now, state, visible } from './state.js';

const rows = new Map();
const groups = new Map();

function stateTime(agent) {
  return agent.kind === 'terminal' ? null : elapsed(agent, now());
}

function aliveTime(agent) {
  return alive(agent) && agent.startedAt !== null ? duration(now() - agent.startedAt) : '';
}

function doing(agent) {
  if (agent.state === 'paused' && !agent.starting) {
    return h('div', { class: 'cell-doing t-yellow' }, 'Waiting for you · ', h('span', { class: 't-text' }, agent.activity));
  }
  const text = activity(agent);
  return h('div', { class: `cell-doing ${text.tone}${agent.kind === 'terminal' ? ' mono' : ''}`, title: text.text }, text.text);
}

function row(agent, index, handlers) {
  const mark = glyph(agent, state.frame);
  const time = stateTime(agent);
  const waiting = agent.state === 'paused' && !agent.starting;
  const classes = ['row', waiting ? 'waiting' : '', agent.state === 'lost' ? 'lost' : ''].filter(Boolean).join(' ');
  return h(
    'div',
    { class: classes, role: 'button', tabindex: '0', dataset: { agent: String(agent.id) }, onclick: () => handlers.open(agent) },
    h('span', { class: 'cell-index' }, String(index + 1)),
    h('span', { class: `cell-glyph glyph ${mark.tone}`, dataset: mark.spin ? { spin: '1' } : {} }, mark.text),
    h('span', { class: `cell-name ${waiting ? 't-yellow' : nameTone(agent)}`, title: agent.name }, agent.name),
    h('span', { class: 'cell-kind', style: `color:${kindColor(agent.kind)}` }, label(agent.kind)),
    h('span', { class: `cell-state ${time ? time.tone : ''}` }, time ? time.text : ''),
    h('span', { class: 'cell-alive' }, aliveTime(agent)),
    doing(agent),
    worktreeCell(agent),
    tools(agent),
  );
}

function worktreeCell(agent) {
  const parts = changeParts(agent.changes);
  const test = testMark(agent.test);
  if (!agent.branch) return h('div', { class: 'cell-worktree' });
  if (!parts) return h('div', { class: 'cell-worktree' }, h('span', { class: 'branch', title: agent.branch }, svg(ICON_BRANCH), agent.branch), test ? h('span', { class: test.tone }, test.text) : null);
  return h('div', { class: 'cell-worktree', title: agent.branch }, ...parts.map((part) => h('span', { class: part.tone }, part.text)), test ? h('span', { class: test.tone }, test.text) : null);
}

function subRow(sub) {
  const line = subagentLine(sub, now());
  return h(
    'div',
    { class: 'row sub' },
    h('span', { class: 'cell-index' }),
    h('span', { class: 'cell-glyph t-faint' }, '↳'),
    h('span', { class: 'cell-name t-muted' }, line.role),
    h('span', { class: 'cell-kind' }),
    h('span', { class: 'cell-state t-faint' }, line.time),
    h('span', { class: 'cell-alive' }),
    h('div', { class: 'cell-doing t-muted' }, sub.description || '', sub.activity ? h('span', { class: 't-faint mono-inline' }, ` · ${sub.activity}`) : null),
    h('div', { class: 'cell-worktree' }),
  );
}

function groupHead(group) {
  return h(
    'div',
    { class: 'list-group' },
    h('span', { class: 'swatch', style: `background:${group.color}` }),
    h('span', { class: 'list-project', style: `--project:${group.color}` }, group.project),
    h('span', { class: 'list-count' }, String(group.agents.length)),
  );
}

function rowSignature(agent, index) {
  return JSON.stringify([index, agent.name, agent.kind, agent.state, agent.starting, agent.activity, agent.branch, agent.exit, agent.changes, agent.test]);
}

export function renderList(handlers) {
  let index = 0;
  const nodes = [];
  const shown = groupByProject(visible());
  for (const group of shown) {
    nodes.push(keep(groups, group.project, `${group.color}:${group.agents.length}`, () => groupHead(group)));
    for (const agent of group.agents) {
      const at = index;
      nodes.push(keep(rows, agent.id, rowSignature(agent, at), () => row(agent, at, handlers)));
      for (const sub of agent.subagents || []) nodes.push(subRow(sub));
      index += 1;
    }
  }
  place($('list-body'), nodes);
  const ids = new Set(shown.flatMap((group) => group.agents.map((agent) => agent.id)));
  for (const id of rows.keys()) if (!ids.has(id)) rows.delete(id);
  const projects = new Set(shown.map((group) => group.project));
  for (const project of groups.keys()) if (!projects.has(project)) groups.delete(project);
}

export function tickList() {
  const byId = new Map(visible().map((agent) => [String(agent.id), agent]));
  for (const node of $('list-body').querySelectorAll('.row[data-agent]')) {
    const agent = byId.get(node.dataset.agent);
    if (!agent) continue;
    const time = stateTime(agent);
    node.querySelector('.cell-state').textContent = time ? time.text : '';
    node.querySelector('.cell-alive').textContent = aliveTime(agent);
  }
}
