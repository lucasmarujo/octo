import { mark, spinner, usesKai } from './brand.js';
import { $, h, svg } from './dom.js';
import { SPINNER, activity, elapsed, glyph, kindColor, kindMark, nameTone, plural, usageOrder, usageShown } from './format.js';
import { applyStatic, language, languageStale, t } from './i18n.js';
import { providerIcon } from './icons.js';
import { islandAgents, islandCounts, islandGroups, islandShape } from './islandview.js';
import { glyph as kai, moodOf } from './kai.js';
import { applyTheme } from './theme.js';
import { channel, invoke } from './transport.js';
import { ringTip, usageRing } from './usageview.js';

const SPIN_MS = 100;
const TICK_MS = 1000;
const HOVER_EVENT = 'island-hover';
const LAYOUT_EVENT = 'island-layout';
const PAUSED_MARK = '‖';
const DONE_MARK = '✓';
const IDLE_MARK = '●';
const MAX_STAGGER = 12;
const START_AFTER_PX = 6;

const view = { overview: null, usage: [], skew: 0, frame: 0, open: false, notch: null, layout: null, shape: '', spinning: 0 };
let primaryNotch;

const now = () => Date.now() - view.skew;
const shown = () => (view.overview ? islandAgents(view.overview.agents) : []);

function brand(mood) {
  if (usesKai()) return kai(mood);
  return mood === 'working' ? spinner() : mark();
}

function chip(tone, sign, count, title, spin) {
  return h('span', { class: `island-chip ${tone}`, title }, h('span', { class: 'glyph', dataset: spin ? { spin: '1' } : {} }, sign), String(count));
}

function chips(counts) {
  const needs = counts.needs === 1 ? t('{count} needs you', { count: counts.needs }) : t('{count} need you', { count: counts.needs });
  const list = [
    counts.working ? chip('t-working', SPINNER[view.frame % SPINNER.length], counts.working, t('{count} working', { count: counts.working }), true) : null,
    counts.needs ? chip('t-yellow', PAUSED_MARK, counts.needs, needs) : null,
    counts.done ? chip('t-green', DONE_MARK, counts.done, t('{count} done', { count: counts.done })) : null,
  ].filter(Boolean);
  if (!list.length && counts.idle) list.push(chip('t-faint', IDLE_MARK, counts.idle, t('{count} idle', { count: counts.idle })));
  return list;
}

function paint(host, signature, build) {
  if (host.dataset.signature === signature) return;
  host.dataset.signature = signature;
  host.replaceChildren(...build());
}

function usageRings(entries) {
  const usage = usageShown(entries, false);
  return usage.length ? h('span', { class: 'island-usage-rings' }, usage.map((entry) => {
    const ring = usageRing(entry);
    const tip = ringTip(entry, now());
    ring.setAttribute('aria-label', tip);
    ring.title = tip;
    return ring;
  })) : null;
}

function renderPill(counts, mood, look, entries) {
  paint($('island-pill'), JSON.stringify([counts, mood, look, entries]), () => [
    h('span', { class: 'island-leading' },
      h('span', { class: 'island-mark' }, brand(mood)),
      usageRings(entries),
    ),
    view.notch ? h('span', { class: 'island-gap' }) : null,
    h('span', { class: 'island-chips' }, chips(counts)),
  ].filter(Boolean));
}

function summary(counts, projects) {
  if (!counts.total) return t('No agents yet');
  return t('{agents} across {projects}', { agents: plural(counts.total, t('agent'), t('agents')), projects: plural(projects, t('project'), t('projects')) });
}

function renderHead(counts, mood, look, projects, entries) {
  paint($('island-head'), JSON.stringify([counts, mood, look, projects, entries]), () => [
    h('span', { class: 'island-mark' }, brand(mood)),
    h('span', { class: 'island-heading' }, h('span', { class: 'island-title-line' }, h('span', { class: 'island-title' }, 'Octo'), usageRings(entries)), h('span', { class: 'island-sub' }, summary(counts, projects))),
    h('span', { class: 'island-chips' }, chips(counts)),
  ]);
}

function icon(kind) {
  const provider = providerIcon(kind);
  return h('span', { class: 'summary-icon' }, provider ? svg(provider) : h('span', { class: 'summary-mark', style: `color:${kindColor(kind)}` }, kindMark(kind)));
}

function time(agent) {
  const spent = elapsed(agent, now());
  return h('span', { class: `island-time ${spent ? spent.tone : ''}`, dataset: { agent: String(agent.id) } }, spent ? spent.text : '');
}

function row(agent, index) {
  const sign = glyph(agent, view.frame);
  const doing = activity(agent);
  return h(
    'button',
    { class: 'island-row', type: 'button', style: `--i:${Math.min(index, MAX_STAGGER)}`, title: [agent.name, doing.text].filter(Boolean).join(' · '), onclick: () => open(agent.id) },
    h('span', { class: `glyph ${sign.tone}`, dataset: sign.spin ? { spin: '1' } : {} }, sign.text),
    icon(agent.kind),
    h('span', { class: `island-name ${nameTone(agent)}` }, agent.name),
    h('span', { class: `island-doing ${doing.tone}` }, doing.text || ''),
    time(agent),
  );
}

function renderList(layout) {
  const signature = JSON.stringify([layout.more, layout.groups.map((group) => [group.project, group.color, group.agents.map((agent) => [agent.id, agent.name, agent.kind, agent.state, agent.starting, agent.activity, agent.exit])])]);
  paint($('island-list'), signature, () => {
    let index = 0;
    const groups = layout.groups.map((group) =>
      h(
        'section',
        { class: 'island-group' },
        h('span', { class: 'island-project caps' }, h('span', { class: 'island-swatch', style: `background:${group.color}` }), h('span', { class: 'island-project-name' }, group.project)),
        group.agents.map((agent) => row(agent, index++)),
      ),
    );
    return layout.more ? [...groups, h('span', { class: 'island-more' }, t('and {count} more', { count: layout.more }))] : groups;
  });
  $('island-list').hidden = !layout.groups.length;
}

function tickTimes() {
  const agents = new Map(shown().map((agent) => [String(agent.id), agent]));
  for (const node of $('island-list').querySelectorAll('.island-time')) {
    const spent = agents.has(node.dataset.agent) ? elapsed(agents.get(node.dataset.agent), now()) : null;
    if (spent && node.textContent !== spent.text) node.textContent = spent.text;
  }
}

function fit() {
  if (!view.layout) return;
  const [island, pill, panel] = [$('island'), $('island-pill'), $('island-panel')];
  const shape = islandShape(
    { width: pill.offsetWidth, height: pill.offsetHeight },
    { width: panel.offsetWidth, height: panel.offsetHeight },
    view.notch, view.open, view.layout, { width: window.innerWidth, height: window.innerHeight },
  );
  const { x, y, width, height } = shape.shown;
  Object.assign(island.style, { left: `${x}px`, top: `${y}px`, width: `${width}px`, height: `${height}px` });
  const reported = JSON.stringify([shape.collapsed, shape.expanded]);
  if (reported !== view.shape) {
    view.shape = reported;
    invoke('island_shape', { shape: { collapsed: shape.collapsed, expanded: shape.expanded } }).catch(() => {});
  }
  if (!island.hasAttribute('data-ready')) requestAnimationFrame(() => island.setAttribute('data-ready', ''));
}

function spin() {
  view.frame += 1;
  const text = SPINNER[view.frame % SPINNER.length];
  for (const node of document.querySelectorAll('[data-spin]')) node.textContent = text;
}

function keepSpinning(on) {
  if (on && !view.spinning) view.spinning = setInterval(spin, SPIN_MS);
  if (!on && view.spinning) {
    clearInterval(view.spinning);
    view.spinning = 0;
  }
}

function render() {
  const agents = shown();
  const counts = islandCounts(agents);
  const mood = moodOf(agents);
  const layout = islandGroups(agents);
  const look = [usesKai(), document.documentElement.dataset.theme, language()];
  const usage = usageOrder(view.usage, agents.map((agent) => agent.kind));
  $('island').dataset.mood = mood;
  renderPill(counts, mood, look, usage);
  renderHead(counts, mood, look, layout.projects, usage);
  renderList(layout);
  keepSpinning(counts.working > 0);
  fit();
}

function open(agent) {
  invoke('island_open', { agent }).catch(() => {});
}

function setOpen(on) {
  view.open = on;
  $('island').dataset.open = String(on);
  if (on) {
    applyTheme();
    tickTimes();
  } else {
    $('island-list').scrollTop = 0;
  }
  render();
}

function setLayout(layout) {
  view.layout = layout;
  view.shape = '';
  view.notch = layout.notch ? primaryNotch : null;
  const island = $('island');
  if (island.hasAttribute('data-ready')) {
    island.setAttribute('data-docking', '');
    requestAnimationFrame(() => requestAnimationFrame(() => island.removeAttribute('data-docking')));
  }
  island.dataset.edge = layout.edge;
  document.documentElement.toggleAttribute('data-notch', Boolean(view.notch));
  render();
}

function initDrag(island) {
  let press;
  let dragged = false;
  island.addEventListener('pointerdown', (event) => {
    if (press || event.button !== 0) return;
    dragged = false;
    const handle = event.target.closest('#island-pill, #island-head');
    if (!handle) return;
    handle.setPointerCapture(event.pointerId);
    press = { handle, id: event.pointerId, x: event.clientX, y: event.clientY };
  });
  window.addEventListener('pointermove', (event) => {
    if (!press || event.pointerId !== press.id || press.started) return;
    if (Math.hypot(event.clientX - press.x, event.clientY - press.y) < START_AFTER_PX) return;
    island.setPointerCapture(event.pointerId);
    island.setAttribute('data-dragging', '');
    press.started = invoke('island_drag', { action: 'start' }).catch(() => {});
  });
  const finish = (event) => {
    if (!press || event.pointerId !== press.id) return;
    const { started } = press;
    press = null;
    if (!started) return;
    island.removeAttribute('data-dragging');
    dragged = event.type === 'pointerup';
    const action = dragged ? 'drop' : 'cancel';
    started.then(() => invoke('island_drag', { action })).catch(() => {});
  };
  island.addEventListener('click', (event) => {
    if (!dragged || event.detail === 0) return;
    dragged = false;
    event.stopImmediatePropagation();
  }, true);
  window.addEventListener('pointerup', finish);
  window.addEventListener('pointercancel', finish);
  island.addEventListener('lostpointercapture', (event) => {
    if (event.target === (press?.started ? island : press?.handle)) finish(event);
  });
}

function onMessage(message) {
  if (message.type === 'usage') {
    view.usage = message.entries;
    render();
    return;
  }
  if (message.type !== 'state') return;
  view.skew = Date.now() - message.now;
  view.overview = message;
  render();
}

function onStorage() {
  if (languageStale()) window.location.reload();
  else {
    applyTheme();
    render();
  }
}

async function start() {
  applyStatic();
  applyTheme();
  window.matchMedia('(prefers-color-scheme: light)').addEventListener('change', onStorage);
  window.addEventListener('storage', onStorage);
  new MutationObserver(render).observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme', 'data-kai'] });
  document.addEventListener('contextmenu', (event) => event.preventDefault());
  $('island-head').addEventListener('click', () => open(null));
  initDrag($('island'));
  primaryNotch = await invoke('island_notch').catch(() => null);
  if (primaryNotch) {
    document.documentElement.style.setProperty('--notch-width', `${primaryNotch.width}px`);
    document.documentElement.style.setProperty('--notch-height', `${primaryNotch.height}px`);
  }
  await window.__TAURI__.event.listen(LAYOUT_EVENT, (event) => setLayout(event.payload));
  await window.__TAURI__.event.listen(HOVER_EVENT, (event) => setOpen(Boolean(event.payload)));
  setLayout(await invoke('island_layout').catch(() => ({ edge: 'top', offset: window.innerWidth / 2, notch: true })));
  window.addEventListener('resize', fit);
  setInterval(() => view.open && tickTimes(), TICK_MS);
  await invoke('island_attach', { channel: channel(onMessage) });
  if (document.fonts) document.fonts.ready.then(fit);
}

start();
