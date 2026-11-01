import { nextAttention, rename, renderAgent, renderAgentHead, renderStatus, restartWhenStopped } from './agent.js';
import { detectAlerts, notice, windowTitle } from './alerts.js';
import { openProjectMenu, renderBoard, renderHead, spinBoard, stopOrRemove, tickBoard } from './board.js';
import { initComposer, renderComposer, renderPalette, setCommandRunner } from './composer.js';
import { runCommand } from './commands.js';
import { closeMenu, confirm, showCompare, showDiff, showFolders, showTests } from './dialogs.js';
import { $, h, svg } from './dom.js';
import { SPINNER, alive, glyph } from './format.js';
import { deactivateGrid, peekRequest, renderGrid, rowsOnScreen, tickGrid } from './grid.js';
import { renderHelp } from './help.js';
import { applyStatic, hubText, language, pageToReopen, t } from './i18n.js';
import { ICON_HELP, ICON_OVERVIEW, ICON_PAIR, ICON_SETTINGS } from './icons.js';
import { glyph as kai, moodOf } from './kai.js';
import { renderList, tickList } from './list.js';
import { openPairing } from './pair.js';
import { applyTheme, renderSettings } from './settings.js';
import { promptNewSession, renderSidebar, spinSidebar } from './sidebar.js';
import { findAgent, inSession, saveTabs, selectSession, setGridSize, setView, state, visible } from './state.js';
import { Terminal } from './terminal.js';
import { toast } from './toast.js';
import { channel, connect, currentWindow, invoke, send } from './transport.js';

const SPIN_MS = 100;
const TICK_MS = 1000;
const MAX_TAB_KEY = 9;
const PEEK_MS = 1000;
const PEEK_STALE_MS = 10_000;
const DETAILS_MS = 3000;
const RENDERERS = { board: [renderBoard, tickBoard], list: [renderList, tickList], grid: [renderGrid, tickGrid] };
const PAGES = {
  help: { label: t('How to use'), icon: ICON_HELP, render: renderHelp },
  settings: { label: t('Settings'), icon: ICON_SETTINGS, render: renderSettings },
};
const VIEWS = ['overview', 'agent-view', 'help-view', 'settings-view'];

const handlers = { open: (agent) => openAgent(agent), term: terminalHandlers(gridShortcut) };
let terminal = null;
let watchedState = null;
let pageAfterReload = null;
const alerting = { previous: new Map(), last: new Map(), title: null };

function sessionNames() {
  return state.overview ? state.overview.sessions.map((session) => session.name) : [];
}

function settleSession() {
  const names = sessionNames();
  if (state.pendingSession && names.includes(state.pendingSession)) {
    selectSession(state.pendingSession);
    state.pendingSession = null;
  }
  if (!state.session || (!names.includes(state.session) && state.session !== state.pendingSession)) {
    selectSession(state.overview.session || names[0] || null);
  }
}

const isPage = (id) => Object.hasOwn(PAGES, id);
const watching = () => state.active !== null && !isPage(state.active);

function settleTabs() {
  const kept = state.tabs.filter((id) => isPage(id) || findAgent(id));
  if (kept.length !== state.tabs.length) {
    state.tabs = kept;
    saveTabs();
  }
  if (watching() && !findAgent(state.active)) showOverview();
}

function onSelect(name) {
  selectSession(name);
  if (state.active !== null) showOverview();
  else onFilter();
}

function closer(id, name) {
  return h(
    'span',
    {
      class: 'tab-close',
      role: 'button',
      'aria-label': t('Close {name}', { name }),
      onclick: (event) => {
        event.stopPropagation();
        closeTab(id);
      },
    },
    '×',
  );
}

function tabButton(id) {
  const classes = `tab${state.active === id ? ' active' : ''}`;
  if (isPage(id)) {
    const page = PAGES[id];
    return h('div', { class: classes, role: 'button', tabindex: '0', onclick: () => openPage(id) }, svg(page.icon), h('span', { class: 'tab-label' }, page.label), closer(id, page.label));
  }
  const agent = findAgent(id);
  if (!agent) return null;
  const mark = glyph(agent, state.frame);
  return h(
    'div',
    { class: classes, role: 'button', tabindex: '0', onclick: () => openAgent(agent) },
    h('span', { class: `tab-glyph ${mark.tone}`, dataset: mark.spin ? { spin: '1' } : {} }, mark.text),
    h('span', { class: 'label-mono' }, agent.name),
    closer(id, agent.name),
  );
}

function renderTabs() {
  const host = $('tabs');
  const signature = JSON.stringify([state.active, state.tabs.map((id) => {
    if (isPage(id)) return id;
    const agent = findAgent(id);
    return agent ? [id, agent.name, agent.state, agent.kind, agent.starting] : null;
  })]);
  if (host.dataset.signature === signature) return;
  host.dataset.signature = signature;
  for (const page of Object.keys(PAGES)) $(`open-${page}`).setAttribute('aria-current', String(state.active === page));
  host.replaceChildren(
    h('button', { class: `tab fixed${state.active === null ? ' active' : ''}`, type: 'button', onclick: showOverview }, svg(ICON_OVERVIEW), t('Overview')),
    ...state.tabs.map(tabButton).filter(Boolean),
  );
}

function renderMascot() {
  const mascot = $('brand-mascot').firstElementChild;
  const mood = moodOf(state.overview ? state.overview.agents : []);
  if (mascot && mascot.dataset.mood !== mood) mascot.dataset.mood = mood;
}

function shownView() {
  return inSession().length ? state.view : 'board';
}

function renderView() {
  const view = shownView();
  if (view !== 'grid') deactivateGrid();
  for (const name of Object.keys(RENDERERS)) $(name).hidden = name !== view;
  RENDERERS[view][0](handlers);
}

function peek() {
  if (!state.overview || state.active !== null || shownView() !== 'grid') return;
  if (state.peekAt && Date.now() - state.peekAt < PEEK_STALE_MS) return;
  const request = peekRequest(visible(), rowsOnScreen());
  if (!request) return;
  state.peekAt = Date.now();
  send(request);
}

function askDetails() {
  if (!watching() || (state.detailsAt && Date.now() - state.detailsAt < PEEK_STALE_MS)) return;
  state.detailsAt = Date.now();
  send({ type: 'details', agent: state.active });
}

function onDetails(details) {
  state.detailsAt = 0;
  state.details = details;
  if (watching() && state.active === details.agent) {
    const agent = findAgent(state.active);
    if (agent) renderAgent(agent);
  }
}

function onPreviews(previews) {
  state.peekAt = 0;
  state.previews = new Map(previews.map((preview) => [preview.agent, preview.lines]));
  if (state.active === null && shownView() === 'grid') renderGrid(handlers);
}

function onFilter() {
  render();
  peek();
}

function render() {
  if (!state.overview) return;
  renderTabs();
  renderMascot();
  renderSidebar(onSelect, openAgent);
  if (state.active === null) {
    renderHead();
    renderView();
    renderComposer();
  } else if (watching()) {
    const agent = findAgent(state.active);
    if (agent) renderAgent(agent);
  }
}

function show(view) {
  for (const id of VIEWS) $(id).hidden = id !== view;
}

function leaveAgent() {
  watchedState = null;
  terminal.close();
}

function openPage(name) {
  if (state.active === name) return;
  deactivateGrid();
  leaveAgent();
  if (!state.tabs.includes(name)) {
    state.tabs.push(name);
    saveTabs();
  }
  state.active = name;
  show(`${name}-view`);
  renderTabs();
  PAGES[name].render();
}

function openTab(id) {
  if (isPage(id)) openPage(id);
  else openAgent(findAgent(id));
}

function openAgent(agent) {
  if (!agent) return;
  deactivateGrid();
  if (agent.session !== state.session) selectSession(agent.session);
  if (!state.tabs.includes(agent.id)) {
    state.tabs.push(agent.id);
    saveTabs();
  }
  const switching = state.active !== agent.id;
  state.active = agent.id;
  state.selected = agent.id;
  show('agent-view');
  render();
  if (switching) {
    state.details = null;
    state.detailsAt = 0;
    askDetails();
    terminal.open(agent.id).catch((err) => toast(t('could not open the terminal: {error}', { error: err }), 'error'));
    watchedState = agent.state;
    if (agent.state === 'done') send({ type: 'seen', agent: agent.id });
  }
  terminal.focus();
}

function showOverview() {
  leaveAgent();
  state.active = null;
  show('overview');
  onFilter();
  $('task').focus();
}

function closeTab(id) {
  state.tabs = state.tabs.filter((tab) => tab !== id);
  saveTabs();
  if (state.active === id) showOverview();
  else renderTabs();
}

function openNextAttention() {
  const next = nextAttention(state.active);
  if (next) openAgent(next);
  else toast(t('No agent is waiting for you.'), 'info');
}

function markSeenWhileWatching() {
  if (!watching()) return;
  const agent = findAgent(state.active);
  if (!agent) return;
  if (agent.state === 'done' && watchedState !== 'done') send({ type: 'seen', agent: agent.id });
  watchedState = agent.state;
}

function raiseAlerts(agents) {
  const { alerts, states } = detectAlerts(alerting.previous, agents, watching() ? state.active : null, Date.now(), alerting.last);
  alerting.previous = states;
  if (alerts.length) {
    for (const alert of alerts) toast(notice(alert).text, notice(alert).tone);
    invoke('alert', { alerts, language: language() }).catch((err) => toast(t('could not alert: {error}', { error: err }), 'warn'));
  }
  const title = windowTitle(agents);
  if (title !== alerting.title) {
    alerting.title = title;
    currentWindow().setTitle(title).catch(() => {});
  }
}

function announceUpdate(version) {
  if (state.updateReady === version) return;
  state.updateReady = version;
  toast(t('Octo {version} is ready · restart the app to use it, your agents keep running', { version }), 'success');
}

async function confirmTrust(message) {
  const sure = await confirm({
    title: t('Run the tests of {repo}?', { repo: message.repo }),
    text: t('After each turn of an agent in a worktree of this repository, Octo runs: {command}', { command: message.command }),
    action: t('Trust'),
  });
  if (sure) send({ type: 'trust', cwd: message.cwd, command: message.command });
}

function phone(arg) {
  if (arg === 'off') {
    invoke('set_phone', { on: false })
      .then(() => toast(t('Octo is off on your phone · ctrl+p turns it on again'), 'info'))
      .catch((err) => toast(String(err), 'error'));
  } else openPairing();
}

const commands = {
  refresh: () => render(),
  focused: () => focusedAgent(),
  selectSession: (name) => onSelect(name),
  newSession: () => promptNewSession(onSelect),
  openAgent: (agent) => openAgent(agent),
  phone,
  help: () => openPage('help'),
  quit: () => currentWindow().close(),
};

function onMessage(message) {
  switch (message.type) {
    case 'welcome':
      state.host = message.host;
      state.version = message.version;
      break;
    case 'state':
      state.skew = Date.now() - message.now;
      state.overview = message;
      settleSession();
      settleTabs();
      markSeenWhileWatching();
      restartWhenStopped(message.agents);
      raiseAlerts(message.agents);
      render();
      reopenTerminal();
      if (pageAfterReload) {
        openPage(pageAfterReload);
        pageAfterReload = null;
      }
      break;
    case 'usage':
      state.usage = message.entries;
      state.usageAt = Date.now();
      renderSidebar(onSelect, openAgent);
      break;
    case 'notice':
      toast(hubText(message.text), message.tone);
      break;
    case 'folders':
      showFolders(message);
      break;
    case 'diff':
      showDiff(message);
      break;
    case 'peek':
      onPreviews(message.previews);
      break;
    case 'details':
      onDetails(message);
      break;
    case 'skills':
      state.skills = { kind: message.kind, cwd: message.cwd, names: message.skills.map((skill) => skill.name), at: Date.now() };
      renderPalette();
      break;
    case 'compare':
      showCompare(message);
      break;
    case 'trustAsk':
      confirmTrust(message);
      break;
    case 'tests':
      showTests(message);
      break;
    default:
      break;
  }
}

function switchSession(step) {
  const names = sessionNames();
  if (names.length < 2) return;
  const at = names.indexOf(state.session);
  onSelect(names[(at + step + names.length) % names.length]);
}

function appShortcut(event) {
  if (event.ctrlKey && !event.altKey && event.key === ']') {
    showOverview();
    return true;
  }
  if (event.altKey && !event.ctrlKey && event.key.toLowerCase() === 'n') {
    openNextAttention();
    return true;
  }
  if (event.altKey && !event.ctrlKey && /^[1-9]$/.test(event.key)) {
    const id = state.tabs[Number(event.key) - 1];
    if (Number(event.key) <= MAX_TAB_KEY && id !== undefined) openTab(id);
    return true;
  }
  if (event.key === 'F1' && !event.ctrlKey && !event.altKey) {
    openPage('help');
    return true;
  }
  if (event.ctrlKey && !event.altKey && event.key.toLowerCase() === 'q') {
    currentWindow().close();
    return true;
  }
  return false;
}

function focusedAgent() {
  const node = document.activeElement && document.activeElement.closest('#overview [data-agent]');
  return node ? findAgent(Number(node.dataset.agent)) : null;
}

function overviewShortcut(event) {
  const plain = event.ctrlKey && !event.altKey && !event.shiftKey;
  const key = event.key.toLowerCase();
  const focused = focusedAgent();
  if (plain && key === 'n') promptNewSession(onSelect);
  else if (plain && key === 'l') $('where').click();
  else if (plain && key === 't') $('worktree').click();
  else if (plain && key === 'r') send({ type: 'recover', session: state.session });
  else if (plain && key === 'p') openPairing();
  else if (plain && key === 'e' && focused) rename(focused);
  else if (event.key === 'Delete' && !event.ctrlKey && !event.altKey && focused) stopOrRemove(focused);
  else if (event.altKey && !event.ctrlKey && event.key === 'ArrowUp') switchSession(-1);
  else if (event.altKey && !event.ctrlKey && event.key === 'ArrowDown') switchSession(1);
  else return false;
  return true;
}

function onShortcut(event) {
  if (event.defaultPrevented || $('dialog').open) return;
  if (event.target instanceof Element && event.target.closest('.xterm')) return;
  if (appShortcut(event)) {
    event.preventDefault();
    return;
  }
  if (state.active !== null) return;
  if (overviewShortcut(event)) event.preventDefault();
}

function copy(text) {
  navigator.clipboard.writeText(text).catch(() => toast(t('Could not copy the selection.'), 'warn'));
}

function openLink(url) {
  invoke('open_link', { url }).catch((err) => toast(String(err), 'warn'));
}

function terminalHandlers(shortcut) {
  return {
    open: (agent, size, onFrame) => invoke('term_open', { agent, rows: size.rows, cols: size.cols, channel: channel(onFrame) }),
    input: (session, bytes) => invoke('term_input', { session, data: Array.from(bytes) }).catch(() => {}),
    resize: (session, size) => invoke('term_resize', { session, rows: size.rows, cols: size.cols }).catch(() => {}),
    close: (session) => invoke('term_close', { session }).catch(() => {}),
    link: openLink,
    copy,
    shortcut,
  };
}

function gridShortcut(event) {
  if (event.ctrlKey && !event.altKey && event.key === ']') {
    deactivateGrid();
    render();
    return true;
  }
  return appShortcut(event);
}

function initTerminal() {
  terminal = new Terminal($('terminal'), terminalHandlers(appShortcut));
}

function reopenTerminal() {
  if (!watching()) return;
  const agent = findAgent(state.active);
  if (agent && alive(agent) && !agent.starting && terminal.stale(agent.id)) {
    terminal.open(agent.id).catch((err) => toast(t('could not open the terminal: {error}', { error: err }), 'error'));
  }
}

function initWindow() {
  const win = currentWindow();
  win.isDecorated().catch(() => false).then((native) => {
    document.documentElement.dataset.windowControls = native ? 'native' : 'app';
  });
  $('win-min').addEventListener('click', () => win.minimize());
  $('win-max').addEventListener('click', () => win.toggleMaximize());
  $('win-close').addEventListener('click', () => win.close());
  $('brand-mascot').replaceChildren(kai('idle'));
  $('open-help').replaceChildren(svg(ICON_HELP));
  $('open-settings').replaceChildren(svg(ICON_SETTINGS));
  $('pair').replaceChildren(svg(ICON_PAIR), t('Pair phone'));
  $('pair').addEventListener('click', openPairing);
  $('open-help').addEventListener('click', () => openPage('help'));
  $('open-settings').addEventListener('click', () => openPage('settings'));
}

function spin() {
  state.frame += 1;
  const text = SPINNER[state.frame % SPINNER.length];
  for (const node of document.querySelectorAll('.tabs [data-spin], .agent-title [data-spin], #list [data-spin], #grid [data-spin], #dialog [data-spin], #details [data-spin]')) node.textContent = text;
  spinBoard();
  spinSidebar();
}

function tick() {
  if (!state.overview) return;
  renderSidebar(onSelect, openAgent);
  if (state.active === null) {
    RENDERERS[shownView()][1]();
    return;
  }
  if (!watching()) return;
  const agent = findAgent(state.active);
  if (agent) {
    renderAgentHead(agent);
    renderStatus(agent);
  }
}

function start() {
  applyStatic();
  pageAfterReload = pageToReopen();
  applyTheme();
  window.matchMedia('(prefers-color-scheme: light)').addEventListener('change', applyTheme);
  initWindow();
  initTerminal();
  initComposer();
  setCommandRunner((text) => runCommand(text, commands));
  $('recover').addEventListener('click', () => send({ type: 'recover', session: state.session }));
  $('project').addEventListener('click', (event) => {
    event.stopPropagation();
    if ($('menu').hidden) openProjectMenu(onFilter);
    else closeMenu();
  });
  for (const button of $('views').children) {
    button.addEventListener('click', () => {
      setView(button.dataset.view);
      onFilter();
    });
  }
  for (const button of $('grid-size').children) {
    button.addEventListener('click', () => {
      setGridSize(button.dataset.size);
      onFilter();
    });
  }
  document.addEventListener('keydown', onShortcut);
  window.addEventListener('orkai:error', (event) => toast(event.detail, 'error'));
  setInterval(spin, SPIN_MS);
  setInterval(tick, TICK_MS);
  setInterval(peek, PEEK_MS);
  setInterval(askDetails, DETAILS_MS);
  invoke('home_dir').then((home) => {
    state.home = home;
  }).catch(() => {});
  invoke('update_ready').then((version) => version && announceUpdate(version)).catch(() => {});
  window.__TAURI__.event.listen('update-ready', (event) => announceUpdate(event.payload));
  connect(onMessage).catch((err) => toast(t('could not reach Octo: {error}', { error: err }), 'error'));
  $('task').focus();
}

start();
