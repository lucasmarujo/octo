import { mark, spinner, usesKai } from './brand.js';
import { $, h, svg } from './dom.js';
import { SPINNER, activity, elapsed, glyph, kindColor, kindMark, nameTone, plural } from './format.js';
import { applyStatic, language, languageStale, t } from './i18n.js';
import { providerIcon } from './icons.js';
import { islandAgents, islandCounts, islandGroups, islandShape } from './islandview.js';
import { glyph as kai, moodOf } from './kai.js';
import { applyTheme } from './theme.js';
import { channel, invoke } from './transport.js';

const SPIN_MS = 100;
const TICK_MS = 1000;
const HOVER_EVENT = 'island-hover';
const PAUSED_MARK = '‖';
const DONE_MARK = '✓';
const IDLE_MARK = '●';
const MAX_STAGGER = 12;

const view = { overview: null, skew: 0, frame: 0, open: false, notch: null, shape: '', spinning: 0 };

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

function renderPill(counts, mood, look) {
  paint($('island-pill'), JSON.stringify([counts, mood, look]), () => [
    h('span', { class: 'island-mark' }, brand(mood)),
    view.notch ? h('span', { class: 'island-gap' }) : null,
    h('span', { class: 'island-chips' }, chips(counts)),
  ].filter(Boolean));
}

function summary(counts, projects) {
  if (!counts.total) return t('No agents yet');
  return t('{agents} across {projects}', { agents: plural(counts.total, t('agent'), t('agents')), projects: plural(projects, t('project'), t('projects')) });
}

function renderHead(counts, mood, look, projects) {
  paint($('island-head'), JSON.stringify([counts, mood, look, projects]), () => [
    h('span', { class: 'island-mark' }, brand(mood)),
    h('span', { class: 'island-heading' }, h('span', { class: 'island-title' }, 'Octo'), h('span', { class: 'island-sub' }, summary(counts, projects))),
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
  const [island, pill, panel] = [$('island'), $('island-pill'), $('island-panel')];
  const shape = islandShape({ width: pill.offsetWidth, height: pill.offsetHeight }, { width: panel.offsetWidth, height: panel.offsetHeight }, view.notch, view.open);
  island.style.width = `${shape.shown.width}px`;
  island.style.height = `${shape.shown.height}px`;
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
  $('island').dataset.mood = mood;
  renderPill(counts, mood, look);
  renderHead(counts, mood, look, layout.projects);
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

function onMessage(message) {
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
  view.notch = await invoke('island_notch').catch(() => null);
  if (view.notch) {
    document.documentElement.toggleAttribute('data-notch', true);
    document.documentElement.style.setProperty('--notch-width', `${view.notch.width}px`);
    document.documentElement.style.setProperty('--notch-height', `${view.notch.height}px`);
  }
  render();
  setInterval(() => view.open && tickTimes(), TICK_MS);
  await window.__TAURI__.event.listen(HOVER_EVENT, (event) => setOpen(Boolean(event.payload)));
  await invoke('island_attach', { channel: channel(onMessage) });
  if (document.fonts) document.fonts.ready.then(fit);
}

start();
