import { deleteSession, renameSession } from './commands.js';
import { $, h, keep, place, svg } from './dom.js';
import { ask, closeMenu, openMenu } from './dialogs.js';
import { SPINNER, USAGE_FOLDED, USAGE_WARN, duration, kindColor, label, usageColor, usageOrder, usageShown } from './format.js';
import { t } from './i18n.js';
import { ICON_CHEVRON_DOWN, ICON_CHEVRON_RIGHT, ICON_PLUS, PROVIDER_ICONS } from './icons.js';
import { agents, inSession, now, selectSession, state, toggleExpanded, toggleUsage } from './state.js';
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

function sessionMenu(event, name) {
  event.preventDefault();
  const item = (text, act) =>
    h(
      'button',
      {
        class: 'menu-item',
        type: 'button',
        role: 'menuitem',
        onclick: () => {
          closeMenu();
          act(name);
        },
      },
      h('span', null, text),
    );
  openMenu(event.currentTarget, [item(t('Rename'), renameSession), item(t('Delete'), deleteSession)], false);
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
      'aria-label': expanded ? t('Collapse') : t('Expand'),
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
    { class: 'session-row', type: 'button', onclick: () => onSelect(session.name), oncontextmenu: (event) => sessionMenu(event, session.name) },
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
  const name = await ask({ title: t('New session'), text: t('A session groups agents you work on together.'), action: t('Create'), placeholder: t('session name') });
  if (!name) return;
  state.pendingSession = name;
  send({ type: 'session', name });
  onSelect(name);
}

const UNFOLD_MS = 320;
const usageCards = new Map();
const usageFrame = new Map();

function usageRow(window) {
  const used = Math.max(0, Math.min(100, window.used));
  return h(
    'div',
    { class: 'usage-row' },
    h(
      'div',
      { class: 'usage-line' },
      h('span', { class: 'usage-label' }, t(window.label)),
      h('span', { class: 'usage-reset', dataset: window.resetsAt ? { resets: String(window.resetsAt) } : {} }),
      h('span', { class: 'usage-pct', style: `color:${window.used >= USAGE_WARN ? usageColor(window.used) : 'var(--color-text)'}` }, `${Math.round(window.used)}%`),
    ),
    h('div', { class: 'usage-bar' }, h('div', { class: 'usage-fill', style: `width:${used}%;--fill:${usageColor(window.used)}` })),
  );
}

function usageCard(entry) {
  const icon = PROVIDER_ICONS[entry.kind];
  const funds = entry.credits !== null && entry.credits !== undefined
    ? h('span', { class: 'usage-funds' }, t('funds {amount}', { amount: `$${entry.credits.toFixed(2)}` }))
    : null;
  return h(
    'div',
    { class: 'usage-card', style: `--tone:${kindColor(entry.kind)}` },
    h('div', { class: 'usage-provider' }, h('span', { class: 'usage-icon' }, icon ? svg(icon) : null), h('span', { class: 'usage-kind' }, label(entry.kind)), funds),
    entry.windows.map(usageRow),
    entry.problem ? h('span', { class: 'usage-note' }, t(entry.problem)) : null,
  );
}

function unfold() {
  const host = $('usage');
  host.classList.add('unfolding');
  setTimeout(() => host.classList.remove('unfolding'), UNFOLD_MS);
}

function usageHead(total, hidden) {
  const open = state.usageOpen;
  const toggle = total > USAGE_FOLDED
    ? h(
        'button',
        {
          class: 'usage-toggle',
          type: 'button',
          'aria-expanded': String(open),
          title: open ? t('Show fewer') : t('Show {count} more', { count: hidden }),
          onclick: () => {
            toggleUsage();
            if (state.usageOpen) unfold();
            renderUsage();
          },
        },
        open ? null : h('span', null, `+${hidden}`),
        svg(ICON_CHEVRON_DOWN),
      )
    : null;
  return h('div', { class: 'usage-head' }, h('span', { class: 'caps' }, t('Plan usage')), h('span', { class: 'usage-note usage-ago' }), toggle);
}

function renderUsage() {
  const host = $('usage');
  const entries = usageOrder(state.usage || [], agents().map((agent) => agent.kind));
  if (!entries.length) {
    host.hidden = true;
    return;
  }
  host.hidden = false;
  const shown = usageShown(entries, state.usageOpen);
  const hidden = entries.length - shown.length;
  const head = keep(usageFrame, 'head', JSON.stringify([state.usageOpen, entries.length, hidden]), () => usageHead(entries.length, hidden));
  head.querySelector('.usage-ago').textContent = state.usageAt ? t('{time} ago', { time: duration(Date.now() - state.usageAt) }) : '';
  const cards = shown.map((entry, index) => {
    const card = keep(usageCards, entry.kind, JSON.stringify(entry), () => usageCard(entry));
    card.classList.toggle('extra', index >= USAGE_FOLDED);
    return card;
  });
  const list = keep(usageFrame, 'list', '', () => h('div', { class: 'usage-list' }));
  place(list, cards);
  place(host, [head, list]);
  host.classList.toggle('open', state.usageOpen);
  for (const node of host.querySelectorAll('[data-resets]')) {
    node.textContent = t('resets in {time}', { time: duration(Number(node.dataset.resets) - now()) });
  }
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
      h('span', { class: 'session-name' }, t('New session')),
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
