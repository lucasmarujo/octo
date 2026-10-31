import { $, h, svg } from './dom.js';
import { ask } from './dialogs.js';
import { SPINNER, duration, kindColor, label, usageColor } from './format.js';
import { ICON_CHEVRON_DOWN, ICON_CHEVRON_RIGHT, ICON_PLUS } from './icons.js';
import { inSession, now, selectSession, state, toggleExpanded } from './state.js';
import { send } from './transport.js';

const SUMMARY = [
  { state: 'paused', glyph: '‖', tone: 't-yellow', waiting: true },
  { state: 'working', glyph: null, tone: 't-text' },
  { state: 'done', glyph: '✓', tone: 't-green' },
];

function summaryLine(entry, list, onOpen) {
  if (!list.length) return null;
  const glyph = entry.glyph || SPINNER[state.frame % SPINNER.length];
  const rest = list.length > 1 ? h('span', { class: 't-faint' }, ` +${list.length - 1}`) : null;
  return h(
    'div',
    { class: `summary${entry.waiting ? ' waiting' : ''}`, onclick: () => onOpen(list[0]) },
    h('span', { class: `glyph ${entry.tone}`, dataset: entry.glyph ? {} : { spin: '1' } }, glyph),
    h('span', { class: 'summary-text' }, list[0].name, rest),
  );
}

function sessionItem(session, onSelect, onOpen) {
  const agents = inSession(session.name);
  const selected = session.name === state.session;
  const expanded = state.expanded.has(session.name);
  const working = agents.filter((agent) => agent.state === 'working').length;
  const chevron = h(
    'span',
    {
      class: 'chevron',
      role: 'button',
      'aria-label': expanded ? 'Collapse' : 'Expand',
      onclick: (event) => {
        event.stopPropagation();
        toggleExpanded(session.name);
        renderSidebar(onSelect, onOpen);
      },
    },
    svg(expanded ? ICON_CHEVRON_DOWN : ICON_CHEVRON_RIGHT),
  );
  const hint = !expanded && working ? h('span', { class: 'session-hint' }, `${SPINNER[1]}${working}`) : null;
  const row = h(
    'button',
    { class: 'session-row', type: 'button', onclick: () => onSelect(session.name) },
    chevron,
    h('span', { class: 'session-name' }, session.name),
    hint,
    h('span', { class: 'session-count' }, String(session.agents)),
  );
  const lines = expanded
    ? SUMMARY.map((entry) => summaryLine(entry, agents.filter((agent) => agent.state === entry.state && !agent.starting), onOpen))
    : [];
  const visible = lines.filter(Boolean);
  const classes = ['session', selected ? 'selected' : '', expanded && visible.length ? 'expanded' : ''].filter(Boolean);
  return h('div', { class: classes.join(' ') }, row, visible);
}

async function newSession(onSelect) {
  const name = await ask({ title: 'New session', text: 'A session groups agents you work on together.', action: 'Create', placeholder: 'session name' });
  if (!name) return;
  state.pendingSession = name;
  send({ type: 'session', name });
  onSelect(name);
}

function usageRow(window) {
  const used = Math.max(0, Math.min(100, window.used));
  const reset = window.resetsAt ? duration(window.resetsAt - now()) : '';
  return h(
    'div',
    { class: 'usage-row' },
    h('span', { class: 'usage-label' }, window.label),
    h('div', { class: 'usage-bar' }, h('div', { class: 'usage-fill', style: `width:${used}%;background:${usageColor(window.used)}` })),
    h('span', { class: 'usage-pct', style: `color:${window.used >= 70 ? usageColor(window.used) : 'var(--color-text)'}` }, `${Math.round(window.used)}%`),
    h('span', { class: 'usage-reset' }, reset),
  );
}

function renderUsage() {
  const host = $('usage');
  const entries = state.usage || [];
  if (!entries.length) {
    host.hidden = true;
    return;
  }
  host.hidden = false;
  const ago = state.usageAt ? `${duration(Date.now() - state.usageAt)} ago` : '';
  const blocks = entries.map((entry) => {
    const rows = entry.windows.map(usageRow);
    if (entry.credits !== null && entry.credits !== undefined) {
      rows.push(h('span', { class: 'usage-note' }, `funds $${entry.credits.toFixed(2)}`));
    }
    if (entry.problem) rows.push(h('span', { class: 'usage-note' }, entry.problem));
    return h(
      'div',
      { class: 'usage-agent' },
      h('span', { class: 'usage-kind', style: `color:${kindColor(entry.kind)}` }, label(entry.kind)),
      rows,
    );
  });
  host.replaceChildren(
    h('div', { class: 'usage-head' }, h('span', { class: 'caps' }, 'Plan usage'), h('span', { class: 'usage-note' }, ago)),
    ...blocks,
  );
}

export function renderSidebar(onSelect, onOpen) {
  $('host').textContent = state.host;
  const overview = state.overview;
  const host = $('sessions');
  const signature = overview
    ? JSON.stringify([
        state.session,
        [...state.expanded],
        overview.sessions.map((session) => [session.name, session.agents, inSession(session.name).map((agent) => [agent.name, agent.state, agent.starting])]),
      ])
    : '';
  if (overview && host.dataset.signature !== signature) {
    host.dataset.signature = signature;
    const items = overview.sessions.map((session) => sessionItem(session, onSelect, onOpen));
    const create = h(
      'button',
      { class: 'session-row new-session', type: 'button', onclick: () => newSession(onSelect) },
      h('span', { class: 'chevron' }, svg(ICON_PLUS)),
      h('span', { class: 'session-name' }, 'New session'),
      h('span', { class: 'kbd' }, 'ctrl N'),
    );
    host.replaceChildren(...items, create);
  }
  renderUsage();
}

export function spinSidebar() {
  for (const node of $('sessions').querySelectorAll('[data-spin]')) node.textContent = SPINNER[state.frame % SPINNER.length];
}

export function promptNewSession(onSelect) {
  return newSession(onSelect);
}
