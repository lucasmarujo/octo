import { deleteSession, renameSession } from './commands.js';
import { $, h, keep, place, svg } from './dom.js';
import { ask, closeMenu, confirm, openMenu } from './dialogs.js';
import { SPINNER, USAGE_FOLDED, USAGE_WARN, duration, kindColor, label, usageColor, usageOrder, usageShown } from './format.js';
import { t } from './i18n.js';
import { ICON_CHEVRON_DOWN, ICON_CHEVRON_RIGHT, ICON_FOLDER, ICON_MORE, ICON_PLUS, PROVIDER_ICONS } from './icons.js';
import { assignWorkspace, agents, createWorkspace, inSession, moveWorkspace, now, removeWorkspace, renameWorkspace, state, toggleExpanded, toggleUsage, toggleWorkspace, workspaceFor } from './state.js';
import { motion } from './motion.js';
import { toast } from './toast.js';
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

let select = null;
let open = null;
let dragging = null;

function refresh() {
  $('sessions').dataset.signature = '';
  renderSidebar(select, open);
}

function menuItem(text, act) {
  return h('button', { class: 'menu-item', type: 'button', role: 'menuitem', onclick: () => { closeMenu(); act(); } }, h('span', null, text));
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
  const workspace = workspaceFor(name);
  const siblings = workspace ? workspace.sessions.filter((session) => state.overview.sessions.some((item) => item.name === session)) : [];
  const index = siblings.indexOf(name);
  const move = (id, before) => () => { assignWorkspace(name, id, before); refresh(); };
  if (state.workspaces.length) {
    const destinations = [{ id: null, name: t('Uncategorized') }, ...state.workspaces].filter((item) => item.id !== (workspace?.id || null));
    openMenu(event.currentTarget, [
      item(t('Rename'), renameSession),
      workspace && index > 0 ? menuItem(t('Move up'), move(workspace.id, siblings[index - 1])) : null,
      workspace && index < siblings.length - 1 ? menuItem(t('Move down'), move(workspace.id, siblings[index + 2] || null)) : null,
      ...destinations.map((item) => menuItem(t('Move to {name}', { name: item.name }), move(item.id, null))),
      item(t('Delete'), deleteSession),
    ].filter(Boolean), false);
    return;
  }
  openMenu(event.currentTarget, [item(t('Rename'), renameSession), item(t('Delete'), deleteSession)], false);
}

async function newWorkspace() {
  const name = await ask({ title: t('New workspace'), text: t('Group related sessions in a collapsible category.'), action: t('Create'), placeholder: t('workspace name') });
  if (!name) return;
  if (!createWorkspace(name)) return toast(t('A workspace with this name already exists.'), 'warn');
  refresh();
}

function workspaceMenu(event, workspace) {
  event.preventDefault();
  const index = state.workspaces.indexOf(workspace);
  const items = [menuItem(t('New session in {name}', { name: workspace.name }), () => newSession(select, workspace)),
    menuItem(t('Rename workspace'), async () => {
      const name = await ask({ title: t('Rename workspace'), value: workspace.name, action: t('Rename') });
      if (name && !renameWorkspace(workspace.id, name)) toast(t('A workspace with this name already exists.'), 'warn');
      refresh();
    }),
    index > 0 ? menuItem(t('Move up'), () => { moveWorkspace(workspace.id, state.workspaces[index - 1].id); refresh(); }) : null,
    index < state.workspaces.length - 1 ? menuItem(t('Move down'), () => { moveWorkspace(workspace.id, state.workspaces[index + 2]?.id); refresh(); }) : null,
    menuItem(t('Remove workspace'), async () => {
      const sure = await confirm({ title: t('Remove workspace'), text: t('Only the category is removed. Its sessions and agents are kept.'), action: t('Remove'), danger: true });
      if (sure) { removeWorkspace(workspace.id); refresh(); }
    }),
  ].filter(Boolean);
  openMenu(event.currentTarget, items, false);
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
    { class: 'session-row', id: `session-${encodeURIComponent(session.name)}`, type: 'button', draggable: true, dataset: { session: session.name }, onclick: () => onSelect(session.name), oncontextmenu: (event) => sessionMenu(event, session.name) },
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
  return h('div', { class: classes.join(' '), dataset: { session: session.name } }, row, visible);
}

async function newSession(onSelect, workspace = workspaceFor()) {
  const name = await ask({ title: t('New session'), text: t('A session groups agents you work on together.'), action: t('Create'), placeholder: t('session name') });
  if (!name) return;
  if (state.overview?.sessions.some((session) => session.name === name)) return toast(t('A session with this name already exists.'), 'warn');
  state.pendingSession = name;
  state.pendingWorkspace = { name, id: workspace?.id || null };
  send({ type: 'session', name });
  onSelect(name);
}

function workspaceGroup(workspace, sessions, onSelect, onOpen) {
  const id = workspace?.id || 'uncategorized';
  const name = workspace?.name || t('Uncategorized');
  const collapsed = workspace ? Boolean(workspace.collapsed) : state.uncategorizedCollapsed;
  const waiting = sessions.flatMap((session) => inSession(session.name)).filter((agent) => agent.state === 'paused' && !agent.starting).length;
  return h('section', { class: 'sidebar-workspace', dataset: { workspace: id } },
    h('div', { class: 'sidebar-workspace-head', draggable: Boolean(workspace),
      oncontextmenu: workspace ? (event) => workspaceMenu(event, workspace) : null },
      h('button', { class: 'sidebar-workspace-toggle', id: `workspace-${id}`, type: 'button', 'aria-expanded': String(!collapsed), onclick: () => { toggleWorkspace(id); refresh(); } },
        svg(workspace ? collapsed ? ICON_CHEVRON_RIGHT : ICON_CHEVRON_DOWN : ICON_FOLDER), h('span', { class: 'sidebar-workspace-name' }, name),
        waiting ? h('span', { class: 'sidebar-workspace-waiting' }, String(waiting)) : h('span', { class: 'session-count' }, String(sessions.length))),
      h('button', { class: 'sidebar-workspace-action', type: 'button', 'aria-label': t('New session in {name}', { name }), title: t('New session in {name}', { name }), onclick: () => newSession(onSelect, workspace) }, svg(ICON_PLUS)),
      workspace ? h('button', { class: 'sidebar-workspace-action', type: 'button', 'aria-label': t('Workspace options'), title: t('Workspace options'), onclick: (event) => workspaceMenu(event, workspace) }, svg(ICON_MORE)) : null,
    ),
    h('div', { class: 'sidebar-workspace-sessions', hidden: collapsed },
      sessions.map((session) => sessionItem(session, onSelect, onOpen)),
      sessions.length ? null : h('button', { class: 'sidebar-workspace-empty', type: 'button', onclick: () => newSession(onSelect, workspace) }, t('Create your first session')),
    ),
  );
}

function listen(host) {
  host.addEventListener('dragstart', (event) => {
    const row = event.target.closest('[data-session]');
    const group = event.target.closest('.sidebar-workspace-head');
    dragging = row ? { kind: 'session', name: row.dataset.session } : group && group.parentElement.dataset.workspace !== 'uncategorized' ? { kind: 'workspace', id: group.parentElement.dataset.workspace } : null;
    if (!dragging) return event.preventDefault();
    event.dataTransfer.effectAllowed = 'move';
    event.dataTransfer.setData('text/plain', dragging.name || dragging.id);
  });
  host.addEventListener('dragover', (event) => {
    const group = event.target.closest('.sidebar-workspace');
    if (!dragging || !group) return;
    event.preventDefault();
    event.dataTransfer.dropEffect = 'move';
    for (const item of host.querySelectorAll('[data-drop]')) delete item.dataset.drop;
    group.dataset.drop = dragging.kind;
  });
  host.addEventListener('drop', (event) => {
    if (!dragging) return;
    const group = event.target.closest('.sidebar-workspace');
    if (!group) return;
    event.preventDefault();
    delete group.dataset.drop;
    const before = new Map([...host.querySelectorAll('.sidebar-workspace, .session')].map((node) => [node.dataset.workspace ? `w:${node.dataset.workspace}` : `s:${node.dataset.session}`, node.getBoundingClientRect().top]));
    if (dragging.kind === 'workspace') {
      if (group.dataset.workspace !== 'uncategorized') {
        const after = event.clientY > group.getBoundingClientRect().top + group.offsetHeight / 2;
        moveWorkspace(dragging.id, after ? group.nextElementSibling?.dataset.workspace || null : group.dataset.workspace);
      }
    } else {
      const id = group.dataset.workspace === 'uncategorized' ? null : group.dataset.workspace;
      const row = event.target.closest('[data-session]');
      if (row?.dataset.session === dragging.name) { dragging = null; return; }
      const after = row && event.clientY > row.getBoundingClientRect().top + row.offsetHeight / 2;
      const next = after ? row.nextElementSibling?.dataset.session : row?.dataset.session;
      assignWorkspace(dragging.name, id, next || null);
    }
    dragging = null;
    refresh();
    const options = motion('--motion-slow');
    for (const node of host.querySelectorAll('.sidebar-workspace, .session')) {
      const from = before.get(node.dataset.workspace ? `w:${node.dataset.workspace}` : `s:${node.dataset.session}`);
      if (from !== undefined && options.duration) { const dy = from - node.getBoundingClientRect().top; if (dy) node.animate([{ transform: `translateY(${dy}px)` }, { transform: 'none' }], options); }
    }
  });
  host.addEventListener('dragend', () => {
    dragging = null;
    for (const item of host.querySelectorAll('[data-drop]')) delete item.dataset.drop;
  });
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
    host.hidden = false;
    host.replaceChildren(h('span', { class: 'usage-note' }, t('No usage data yet.')));
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
  select = onSelect;
  open = onOpen;
  $('host').textContent = state.host;
  if ($('breadcrumb').firstElementChild) $('breadcrumb').firstElementChild.textContent = workspaceFor()?.name || t('Personal workspace');
  const overview = state.overview;
  const host = $('sessions');
  const createWorkspaceButton = $('new-workspace');
  if (!createWorkspaceButton.dataset.ready) {
    createWorkspaceButton.dataset.ready = '1';
    createWorkspaceButton.replaceChildren(svg(ICON_PLUS));
    createWorkspaceButton.addEventListener('click', newWorkspace);
    listen(host);
  }
  const signature = overview
    ? JSON.stringify([
        state.session,
        [...state.expanded],
        state.workspaces,
        state.uncategorizedCollapsed,
        overview.sessions.map((session) => [session.name, session.agents, inSession(session.name).map((agent) => [agent.name, agent.state, agent.starting])]),
      ])
    : '';
  if (overview && !dragging && host.dataset.signature !== signature) {
    host.dataset.signature = signature;
    const focused = host.contains(document.activeElement) ? document.activeElement.id : null;
    const items = state.workspaces.map((workspace) => workspaceGroup(workspace, workspace.sessions.map((name) => overview.sessions.find((session) => session.name === name)).filter(Boolean), onSelect, onOpen));
    const uncategorized = overview.sessions.filter((session) => !workspaceFor(session.name));
    if (uncategorized.length || !items.length) items.push(workspaceGroup(null, uncategorized, onSelect, onOpen));
    const create = h(
      'button',
      { class: 'session-row new-session', type: 'button', onclick: () => newSession(onSelect) },
      h('span', { class: 'chevron' }, svg(ICON_PLUS)),
      h('span', { class: 'session-name' }, t('New session')),
      h('span', { class: 'kbd' }, 'ctrl N'),
    );
    host.replaceChildren(...items, create);
    if (focused) document.getElementById(focused)?.focus({ preventScroll: true });
  }
  renderUsage();
}

export function spinSidebar() {
  for (const node of $('sessions').querySelectorAll('[data-spin]')) node.textContent = SPINNER[state.frame % SPINNER.length];
}

export function promptNewSession(onSelect) {
  return newSession(onSelect);
}
