import { deleteSession, renameSession } from './commands.js';
import { $, h, keep, place, storage, svg } from './dom.js';
import { ask, closeMenu, openMenu } from './dialogs.js';
import { DETAILS_DEFAULT, DETAILS_MAX, DETAILS_MIN, SIDEBAR_DEFAULT, SIDEBAR_MAX, SIDEBAR_MIN, SPINNER, USAGE_FOLDED, USAGE_WARN, agentsTip, detailsWidth, duration, kindColor, kindMark, label, shortModel, sidebarWidth, usageColor, usageOrder, usageShown } from './format.js';
import { t } from './i18n.js';
import { ICON_CHEVRON_DOWN, ICON_CHEVRON_RIGHT, ICON_DETAILS_COLLAPSE, ICON_DETAILS_EXPAND, ICON_PLUS, ICON_SIDEBAR_COLLAPSE, ICON_SIDEBAR_EXPAND, providerIcon } from './icons.js';
import { motionMs } from './motion.js';
import { agents, inSession, now, selectSession, state, toggleExpanded, toggleUsage } from './state.js';
import { send } from './transport.js';

const WIDTH_KEY = 'octo.sidebar';
const DETAILS_WIDTH_KEY = 'octo.details';
const SIDEBAR_COLLAPSED_KEY = 'octo.sidebarCollapsed';
const DETAILS_COLLAPSED_KEY = 'octo.detailsCollapsed';
const WIDTH_STEP = 16;
const DETAILS_HANDLE_GAP = 8;
const SUMMARY = [
  { state: 'paused', glyph: '‖', tone: 't-yellow', waiting: true },
  { state: 'working', glyph: null, tone: 't-text' },
  { state: 'done', glyph: '✓', tone: 't-green' },
];

function summaryIcon(kind) {
  const icon = providerIcon(kind);
  return h('span', { class: 'summary-icon' }, icon ? svg(icon) : h('span', { class: 'summary-mark', style: `color:${kindColor(kind)}` }, kindMark(kind)));
}

function summaryLine(entry, list, onOpen) {
  if (!list.length) return null;
  const glyph = entry.glyph || SPINNER[state.frame % SPINNER.length];
  const rest = list.length > 1 ? h('span', { class: 'summary-rest' }, `+${list.length - 1}`) : null;
  const model = shortModel(list[0].model);
  return h(
    'div',
    { class: `summary${entry.waiting ? ' waiting' : ''}`, dataset: { agent: String(list[0].id) }, title: agentsTip(list), onclick: () => onOpen(list[0]) },
    h('span', { class: `glyph ${entry.tone}`, dataset: entry.glyph ? {} : { spin: '1' } }, glyph),
    summaryIcon(list[0].kind),
    h('span', { class: 'summary-text' }, list[0].name),
    rest,
    model ? h('span', { class: 'summary-model' }, model) : null,
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
    { class: 'session-row', type: 'button', dataset: { session: session.name }, onclick: () => onSelect(session.name), oncontextmenu: (event) => sessionMenu(event, session.name) },
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
const RING_CENTER = 18;
const RING_OUTER = 16.5;
const RING_STEP = 4.5;
const USAGE_RINGS = 2;
const usageCards = new Map();
const usageFrame = new Map();
const usageRings = new Map();

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
  const icon = providerIcon(entry.kind);
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

const usedPercent = (window) => Math.max(0, Math.min(100, window.used));

function ringCircles(window, index) {
  const radius = RING_OUTER - index * RING_STEP;
  const circle = `cx="${RING_CENTER}" cy="${RING_CENTER}" r="${radius}"`;
  const used = usedPercent(window);
  const dash = used < 100 ? ` pathLength="100" stroke-dasharray="${used} 100"` : '';
  return `<circle class="ring-track" ${circle}/><circle class="ring-fill" ${circle}${dash} style="stroke:${usageColor(window.used)}"/>`;
}

function usageRing(entry) {
  const icon = providerIcon(entry.kind);
  const rings = entry.windows.slice(0, USAGE_RINGS).map(ringCircles).join('');
  return h(
    'div',
    { class: 'usage-ring', role: 'img', dataset: { kind: entry.kind } },
    svg(`<svg class="ring" width="36" height="36" viewBox="0 0 36 36" aria-hidden="true">${rings}</svg>`),
    h('span', { class: 'usage-ring-icon' }, icon ? svg(icon) : h('span', { class: 'summary-mark', style: `color:${kindColor(entry.kind)}` }, kindMark(entry.kind))),
  );
}

function ringTip(entry) {
  const lines = entry.windows.map((window) => {
    const resets = window.resetsAt ? ` · ${t('resets in {time}', { time: duration(window.resetsAt - now()) })}` : '';
    return `${t(window.label)} ${Math.round(window.used)}%${resets}`;
  });
  if (entry.credits !== null && entry.credits !== undefined) lines.push(t('funds {amount}', { amount: `$${entry.credits.toFixed(2)}` }));
  if (entry.problem) lines.push(t(entry.problem));
  return [label(entry.kind), ...lines].join('\n');
}

function renderRings(entries) {
  const host = $('usage-rings');
  host.hidden = !entries.length;
  const rings = entries.map((entry) => {
    const ring = keep(usageRings, entry.kind, JSON.stringify(entry), () => usageRing(entry));
    ring.dataset.tip = ringTip(entry);
    ring.setAttribute('aria-label', ring.dataset.tip);
    return ring;
  });
  place(host, rings);
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
    renderRings([]);
    return;
  }
  host.hidden = false;
  const shown = usageShown(entries, state.usageOpen);
  renderRings(shown);
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
        overview.sessions.map((session) => [session.name, session.agents, inSession(session.name).map((agent) => [agent.id, agent.name, agent.state, agent.starting, agent.kind, agent.model])]),
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

function initPanelWidth({ handle, key, property, clamp, fallback, min, max, fromPointer, growKey }) {
  let width = clamp(storage.get(key));
  const apply = (value) => {
    width = clamp(value);
    document.documentElement.style.setProperty(property, `${width}px`);
    handle.setAttribute('aria-valuenow', String(width));
  };
  const save = () => storage.set(key, width === fallback ? null : String(width));
  handle.setAttribute('aria-valuemin', String(min));
  handle.setAttribute('aria-valuemax', String(max));
  apply(width);
  handle.addEventListener('pointerdown', (event) => {
    if (event.button !== 0) return;
    event.preventDefault();
    handle.setPointerCapture(event.pointerId);
    handle.classList.add('resizing');
    document.documentElement.classList.add('resizing-panel');
  });
  handle.addEventListener('pointermove', (event) => {
    if (handle.hasPointerCapture(event.pointerId)) apply(fromPointer(event));
  });
  handle.addEventListener('lostpointercapture', () => {
    handle.classList.remove('resizing');
    document.documentElement.classList.remove('resizing-panel');
    save();
  });
  handle.addEventListener('dblclick', () => {
    apply(fallback);
    save();
  });
  handle.addEventListener('keydown', (event) => {
    if (event.key !== 'ArrowLeft' && event.key !== 'ArrowRight') return;
    event.preventDefault();
    apply(width + (event.key === growKey ? WIDTH_STEP : -WIDTH_STEP));
    save();
  });
}

function initPanelToggle({ button, key, name, collapseText, expandText, collapseIcon, expandIcon, onToggle }) {
  let collapsed = storage.get(key) === 'on';
  let settling = 0;
  const paint = () => {
    const text = collapsed ? expandText : collapseText;
    document.documentElement.dataset[name] = collapsed ? 'collapsed' : 'open';
    button.setAttribute('aria-expanded', String(!collapsed));
    button.setAttribute('aria-label', text);
    button.title = text;
    button.replaceChildren(svg(collapsed ? expandIcon : collapseIcon));
  };
  paint();
  button.addEventListener('click', () => {
    collapsed = !collapsed;
    storage.set(key, collapsed ? 'on' : null);
    clearTimeout(settling);
    const ms = motionMs('--motion-slow');
    paint();
    if (ms) {
      document.documentElement.dataset[name] = collapsed ? 'closing' : 'opening';
      settling = setTimeout(paint, ms);
    }
    onToggle();
  });
}

export function initPanelToggles(onToggle) {
  initPanelToggle({
    button: $('sidebar-toggle'),
    key: SIDEBAR_COLLAPSED_KEY,
    name: 'sidebar',
    collapseText: t('Collapse the sidebar'),
    expandText: t('Expand the sidebar'),
    collapseIcon: ICON_SIDEBAR_COLLAPSE,
    expandIcon: ICON_SIDEBAR_EXPAND,
    onToggle,
  });
  initPanelToggle({
    button: $('details-toggle'),
    key: DETAILS_COLLAPSED_KEY,
    name: 'details',
    collapseText: t('Collapse the agent panel'),
    expandText: t('Expand the agent panel'),
    collapseIcon: ICON_DETAILS_COLLAPSE,
    expandIcon: ICON_DETAILS_EXPAND,
    onToggle,
  });
}

export function initSidebarWidth() {
  initPanelWidth({
    handle: $('sidebar-resize'),
    key: WIDTH_KEY,
    property: '--sidebar',
    clamp: sidebarWidth,
    fallback: SIDEBAR_DEFAULT,
    min: SIDEBAR_MIN,
    max: SIDEBAR_MAX,
    fromPointer: (event) => event.clientX,
    growKey: 'ArrowRight',
  });
}

export function initDetailsWidth() {
  initPanelWidth({
    handle: $('details-resize'),
    key: DETAILS_WIDTH_KEY,
    property: '--details',
    clamp: detailsWidth,
    fallback: DETAILS_DEFAULT,
    min: DETAILS_MIN,
    max: DETAILS_MAX,
    fromPointer: (event) => $('details').getBoundingClientRect().right - event.clientX - DETAILS_HANDLE_GAP,
    growKey: 'ArrowLeft',
  });
}
