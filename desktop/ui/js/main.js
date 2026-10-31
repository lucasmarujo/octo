import { nextAttention, renderAgent, renderAgentHead, renderStatus } from './agent.js';
import { renderBoard, renderHead, spinBoard, tickBoard } from './board.js';
import { initComposer, renderComposer } from './composer.js';
import { showDiff, showFolders } from './dialogs.js';
import { $, h, svg } from './dom.js';
import { SPINNER, glyph } from './format.js';
import { ICON_OVERVIEW } from './icons.js';
import { glyph as kai, moodOf } from './kai.js';
import { promptNewSession, renderSidebar, spinSidebar } from './sidebar.js';
import { findAgent, saveTabs, selectSession, state } from './state.js';
import { Terminal } from './terminal.js';
import { toast } from './toast.js';
import { connect, currentWindow, send } from './transport.js';

const SPIN_MS = 100;
const TICK_MS = 1000;
const MAX_TAB_KEY = 9;

const handlers = { open: (agent) => openAgent(agent) };
let terminal = null;
let watchedState = null;

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

function settleTabs() {
  const kept = state.tabs.filter((id) => findAgent(id));
  if (kept.length !== state.tabs.length) {
    state.tabs = kept;
    saveTabs();
  }
  if (state.active !== null && !findAgent(state.active)) showOverview();
}

function onSelect(name) {
  selectSession(name);
  if (state.active !== null) showOverview();
  else render();
}

function tabButton(id) {
  const agent = findAgent(id);
  if (!agent) return null;
  const mark = glyph(agent, state.frame);
  return h(
    'div',
    { class: `tab${state.active === id ? ' active' : ''}`, role: 'button', tabindex: '0', onclick: () => openAgent(agent) },
    h('span', { class: `tab-glyph ${mark.tone}`, dataset: mark.spin ? { spin: '1' } : {} }, mark.text),
    h('span', { class: 'label-mono' }, agent.name),
    h(
      'span',
      {
        class: 'tab-close',
        role: 'button',
        'aria-label': `Close ${agent.name}`,
        onclick: (event) => {
          event.stopPropagation();
          closeTab(id);
        },
      },
      '×',
    ),
  );
}

function renderTabs() {
  const host = $('tabs');
  const signature = JSON.stringify([state.active, state.tabs.map((id) => {
    const agent = findAgent(id);
    return agent ? [id, agent.name, agent.state, agent.kind, agent.starting] : null;
  })]);
  if (host.dataset.signature === signature) return;
  host.dataset.signature = signature;
  host.replaceChildren(
    h('button', { class: `tab${state.active === null ? ' active' : ''}`, type: 'button', onclick: showOverview }, svg(ICON_OVERVIEW), 'Overview'),
    ...state.tabs.map(tabButton).filter(Boolean),
  );
}

function renderMascot() {
  const mascot = $('brand-mascot').firstElementChild;
  const mood = moodOf(state.overview ? state.overview.agents : []);
  if (mascot && mascot.dataset.mood !== mood) mascot.dataset.mood = mood;
}

function render() {
  if (!state.overview) return;
  renderTabs();
  renderMascot();
  renderSidebar(onSelect, openAgent);
  if (state.active === null) {
    renderHead();
    renderBoard(handlers);
    renderComposer();
  } else {
    const agent = findAgent(state.active);
    if (agent) renderAgent(agent);
  }
}

function show(view) {
  $('overview').hidden = view !== 'overview';
  $('agent-view').hidden = view !== 'agent';
}

function openAgent(agent) {
  if (!agent) return;
  if (agent.session !== state.session) selectSession(agent.session);
  if (!state.tabs.includes(agent.id)) {
    state.tabs.push(agent.id);
    saveTabs();
  }
  const switching = state.active !== agent.id;
  state.active = agent.id;
  show('agent');
  render();
  if (switching) {
    const size = terminal.open(agent.id);
    send({ type: 'open', agent: agent.id, rows: size.rows, cols: size.cols });
    watchedState = agent.state;
    if (agent.state === 'done') send({ type: 'seen', agent: agent.id });
  }
  terminal.focus();
}

function showOverview() {
  if (state.active !== null) send({ type: 'close' });
  state.active = null;
  watchedState = null;
  terminal.close();
  show('overview');
  render();
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
  else toast('No agent is waiting for you.', 'info');
}

function markSeenWhileWatching() {
  if (state.active === null) return;
  const agent = findAgent(state.active);
  if (!agent) return;
  if (agent.state === 'done' && watchedState !== 'done') send({ type: 'seen', agent: agent.id });
  watchedState = agent.state;
}

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
      render();
      break;
    case 'usage':
      state.usage = message.entries;
      state.usageAt = Date.now();
      renderSidebar(onSelect, openAgent);
      break;
    case 'screen':
      terminal.apply(message);
      break;
    case 'notice':
      toast(message.text, message.tone);
      break;
    case 'folders':
      showFolders(message);
      break;
    case 'diff':
      showDiff(message);
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
    if (Number(event.key) <= MAX_TAB_KEY && id !== undefined) openAgent(findAgent(id));
    return true;
  }
  return false;
}

function onShortcut(event) {
  if (event.defaultPrevented || $('dialog').open) return;
  if (appShortcut(event)) {
    event.preventDefault();
    return;
  }
  if (state.active !== null) return;
  if (event.ctrlKey && !event.altKey && event.key.toLowerCase() === 'n') {
    event.preventDefault();
    promptNewSession(onSelect);
  } else if (event.altKey && event.key === 'ArrowUp') {
    event.preventDefault();
    switchSession(-1);
  } else if (event.altKey && event.key === 'ArrowDown') {
    event.preventDefault();
    switchSession(1);
  }
}

function copy(text) {
  navigator.clipboard.writeText(text).catch(() => toast('Could not copy the selection.', 'warn'));
}

function initTerminal() {
  terminal = new Terminal($('terminal'), {
    key: (key) => send({ type: 'key', ...key }),
    paste: (text) => send({ type: 'paste', text }),
    scroll: (lines) => send({ type: 'scroll', lines }),
    resize: (size) => send({ type: 'size', rows: size.rows, cols: size.cols }),
    copy,
    shortcut: appShortcut,
  });
}

function initWindow() {
  const win = currentWindow();
  $('win-min').addEventListener('click', () => win.minimize());
  $('win-max').addEventListener('click', () => win.toggleMaximize());
  $('win-close').addEventListener('click', () => win.close());
  $('brand-mascot').replaceChildren(kai('idle'));
}

function spin() {
  state.frame += 1;
  const text = SPINNER[state.frame % SPINNER.length];
  for (const node of document.querySelectorAll('.tabs [data-spin], .agent-title [data-spin]')) node.textContent = text;
  spinBoard();
  spinSidebar();
}

function tick() {
  if (!state.overview) return;
  renderSidebar(onSelect, openAgent);
  if (state.active === null) {
    tickBoard();
    return;
  }
  const agent = findAgent(state.active);
  if (agent) {
    renderAgentHead(agent);
    renderStatus(agent);
  }
}

function start() {
  initWindow();
  initTerminal();
  initComposer();
  $('recover').addEventListener('click', () => send({ type: 'recover', session: state.session }));
  document.addEventListener('keydown', onShortcut);
  window.addEventListener('orkai:error', (event) => toast(event.detail, 'error'));
  setInterval(spin, SPIN_MS);
  setInterval(tick, TICK_MS);
  connect(onMessage).catch((err) => toast(`could not reach orkai: ${err}`, 'error'));
  $('task').focus();
}

start();
