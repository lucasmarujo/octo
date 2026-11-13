import { nextAttention, rename, renderAgent, renderAgentHead, renderStatus, restartWhenStopped } from './agent.js';
import { checkAgentUpdates, watchAgentUpdates } from './agentupdates.js';
import { detectAlerts, notice, windowTitle } from './alerts.js';
import { openProjectMenu, renderBoard, renderHead, spinBoard, stopOrRemove, tickBoard } from './board.js';
import { mark, usesKai } from './brand.js';
import { initComposer, renderComposer, renderPalette, setCommandRunner, toggleWorktree } from './composer.js';
import { renameSession, runCommand } from './commands.js';
import { closeMenu, confirm, menuShown, showCompare, showDiff, showTests } from './dialogs.js';
import { showFolderMade, showFolderSearch, showFolders } from './folders.js';
import { $, svg } from './dom.js';
import { initAgentDrag } from './drag.js';
import { OCTO_TAB, OVERVIEW_TAB, SPINNER, alive, tabAfterClosing } from './format.js';
import { deactivateGrid, gridAgents, peekRequest, renderGrid, rowsOnScreen, tickGrid } from './grid.js';
import { renderHelp } from './help.js';
import { applyStatic, hubText, language, pageToReopen, t } from './i18n.js';
import { ICON_HELP, ICON_OCTO, ICON_PAIR, ICON_SETTINGS } from './icons.js';
import { glyph as kai, moodOf } from './kai.js';
import { initTooltips } from './motion.js';
import { renderList, tickList } from './list.js';
import { initNotes } from './notes.js';
import { initOcto, renderOcto, renderOctoNav, showActivity, showInstructions, tickOcto } from './octo.js';
import { needsCount, orchestratorView, viewRequest } from './orchestrator.js';
import { openPairing, watchPhone } from './pair.js';
import { applyTheme, initSound, pickSection, renderSettings, setAgentOpener, watchInstalls } from './settings.js';
import { initDetailsWidth, initFlyout, initPanelToggles, initSidebarWidth, promptNewSession, renderSidebar, spinSidebar } from './sidebar.js';
import { findAgent, inSession, saveTabs, selectSession, setGridSize, setView, state, visible } from './state.js';
import { initStatusBar, renderStatusBar } from './statusbar.js';
import { initTabs, renderTabs, tabShown } from './tabs.js';
import { Terminal } from './terminal.js';
import { toast } from './toast.js';
import { channel, connect, currentWindow, invoke, onFileDrop, send } from './transport.js';

const SPIN_MS = 100;
const TICK_MS = 1000;
const MAX_TAB_KEY = 9;
const PEEK_MS = 1000;
const PEEK_STALE_MS = 10_000;
const DETAILS_MS = 3000;
const RENDERERS = { board: [renderBoard, tickBoard], list: [renderList, tickList], grid: [renderGrid, tickGrid] };
const PAGES = {
  octo: { label: t('Octo'), icon: ICON_OCTO, render: renderOcto, badge: () => needsCount(state.orchestrator) },
  help: { label: t('How to use'), icon: ICON_HELP, render: renderHelp },
  settings: { label: t('Settings'), icon: ICON_SETTINGS, render: renderSettings },
};
const VIEWS = ['overview', 'agent-view', 'help-view', 'settings-view', 'octo-view'];

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
    selectSession(names.includes(state.overview.session) ? state.overview.session : names[0] || null);
  }
}

const isPage = (id) => Object.hasOwn(PAGES, id);
const watching = () => state.active !== null && !isPage(state.active);

function settleTabs() {
  const kept = state.tabs.filter((id) => id === OVERVIEW_TAB || isPage(id) || findAgent(id));
  if (kept.length !== state.tabs.length) {
    const open = new Set(kept);
    state.tabs = kept;
    state.pinned = new Set([...state.pinned].filter((id) => open.has(id)));
    saveTabs();
  }
  if (watching() && !findAgent(state.active)) openTab(tabAfterClosing(state.tabs, 0));
  else if (state.active === null && !state.tabs.includes(OVERVIEW_TAB)) openTab(tabAfterClosing(state.tabs, 0));
}

function onSelect(name) {
  selectSession(name);
  if (state.active !== null) showOverview();
  else onFilter();
}

function renderBrand() {
  const host = $('brand-mascot');
  const brand = usesKai() ? 'kai' : 'mark';
  if (host.dataset.brand === brand) return;
  host.dataset.brand = brand;
  host.replaceChildren(usesKai() ? kai('idle') : mark());
}

function renderMascot() {
  renderBrand();
  const working = Boolean(state.overview) && state.overview.agents.some((agent) => agent.starting || agent.state === 'working');
  document.documentElement.toggleAttribute('data-working', working);
  if (!usesKai()) return;
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
  const request = peekRequest(gridAgents(), rowsOnScreen());
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
  renderOctoNav();
  renderStatusBar();
  if (state.active === null) {
    renderHead();
    renderView();
    renderComposer();
  } else if (watching()) {
    const agent = findAgent(state.active);
    if (agent) renderAgent(agent);
  } else if (state.active === OCTO_TAB) {
    renderOcto();
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
  renderOctoNav();
  PAGES[name].render();
}

function openUpdates() {
  pickSection('Updates');
  if (state.active === 'settings') renderSettings();
  else openPage('settings');
}

function openTab(id) {
  if (id === OVERVIEW_TAB) showOverview();
  else if (isPage(id)) openPage(id);
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
  if (!state.tabs.includes(OVERVIEW_TAB)) {
    state.tabs.push(OVERVIEW_TAB);
    saveTabs();
  }
  leaveAgent();
  state.active = null;
  show('overview');
  onFilter();
  $('task').focus();
}

function closeTab(id) {
  const shown = tabShown(id);
  const at = state.tabs.indexOf(id);
  state.tabs = state.tabs.filter((tab) => tab !== id);
  state.pinned.delete(id);
  saveTabs();
  if (shown) openTab(tabAfterClosing(state.tabs, at));
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
  const { alerts, states } = detectAlerts(alerting.previous, agents, watching() && document.hasFocus() ? state.active : null, Date.now(), alerting.last);
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
      send(viewRequest());
      break;
    case 'state':
      state.skew = Date.now() - message.now;
      state.overview = message;
      settleSession();
      settleTabs();
      markSeenWhileWatching();
      restartWhenStopped(message.agents);
      watchInstalls(message.agents);
      watchAgentUpdates(message.agents);
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
      renderStatusBar();
      break;
    case 'orchestrator':
      state.orchestrator = orchestratorView(message);
      renderTabs();
      renderOctoNav();
      renderStatusBar();
      if (state.active === OCTO_TAB) renderOcto();
      break;
    case 'orchestratorInstructions':
      showInstructions(message);
      break;
    case 'orchestratorActivity':
      showActivity(message);
      break;
    case 'notice':
      toast(hubText(message.text), message.tone);
      break;
    case 'folders':
      showFolders(message);
      break;
    case 'folderSearch':
      showFolderSearch(message);
      break;
    case 'folderMade':
      showFolderMade(message);
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
    case 'models':
      state.models[message.kind] = { list: message.models, at: Date.now() };
      renderComposer();
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
    const id = state.tabs.filter((tab) => tab !== OVERVIEW_TAB)[Number(event.key) - 1];
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
  else if (plain && key === 't') toggleWorktree();
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

function pasteImage(bytes) {
  return invoke('paste_image', bytes).catch((error) => {
    toast(t('could not paste the image: {error}', { error: hubText(String(error)) }), 'error');
    return null;
  });
}

function terminalHandlers(shortcut) {
  return {
    open: (agent, size, onFrame) => invoke('term_open', { agent, rows: size.rows, cols: size.cols, channel: channel(onFrame) }),
    input: (session, bytes) => invoke('term_input', { session, data: Array.from(bytes) }).catch(() => {}),
    resize: (session, size) => invoke('term_resize', { session, rows: size.rows, cols: size.cols }).catch(() => {}),
    close: (session) => invoke('term_close', { session }).catch(() => {}),
    link: openLink,
    copy,
    image: pasteImage,
    drops: onFileDrop,
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
  renderBrand();
  new MutationObserver(render).observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme'] });
  $('open-help').replaceChildren(svg(ICON_HELP));
  $('open-settings').replaceChildren(svg(ICON_SETTINGS));
  $('pair').replaceChildren(svg(ICON_PAIR));
  $('pair').addEventListener('click', openPairing);
  watchPhone();
  initSound();
  $('open-help').addEventListener('click', () => openPage('help'));
  $('open-settings').addEventListener('click', () => openPage('settings'));
  initNotes(openUpdates);
  initStatusBar({ attention: openNextAttention, phone: openPairing, updates: openUpdates });
}

function spin() {
  state.frame += 1;
  const text = SPINNER[state.frame % SPINNER.length];
  for (const node of document.querySelectorAll('.tabs [data-spin], #menu [data-spin], .agent-title [data-spin], #list [data-spin], #grid [data-spin], #dialog [data-spin], #details [data-spin], #statusbar [data-spin], #octo-view [data-spin]')) node.textContent = text;
  spinBoard();
  spinSidebar();
}

function tick() {
  if (!state.overview) return;
  renderSidebar(onSelect, openAgent);
  renderStatusBar();
  if (state.active === null) {
    RENDERERS[shownView()][1]();
    return;
  }
  if (state.active === OCTO_TAB) tickOcto();
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
  initSidebarWidth();
  initDetailsWidth();
  initPanelToggles(() => {
    if (watching()) terminal.focus();
  });
  initTabs({ pages: PAGES, open: openTab, close: closeTab });
  initAgentDrag();
  initTerminal();
  initComposer();
  initTooltips();
  initFlyout();
  setCommandRunner((text) => runCommand(text, commands));
  setAgentOpener(openAgent);
  initOcto({ openAgent, openPage });
  $('recover').addEventListener('click', () => send({ type: 'recover', session: state.session }));
  $('session-title').addEventListener('dblclick', () => state.session && renameSession(state.session));
  $('agent-title').addEventListener('dblclick', () => watching() && findAgent(state.active) && rename(findAgent(state.active)));
  $('project').addEventListener('click', (event) => {
    event.stopPropagation();
    if (!menuShown()) openProjectMenu(onFilter);
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
  document.addEventListener('contextmenu', (event) => event.preventDefault());
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
  checkAgentUpdates(openAgent);
  $('task').focus();
}

start();
