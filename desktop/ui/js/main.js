import { renderBoard, renderHead, spinBoard, tickBoard } from './board.js';
import { initComposer, renderComposer } from './composer.js';
import { showFolders } from './dialogs.js';
import { $, h, svg } from './dom.js';
import { ICON_OVERVIEW } from './icons.js';
import { glyph, moodOf } from './kai.js';
import { promptNewSession, renderSidebar, spinSidebar } from './sidebar.js';
import { selectSession, state } from './state.js';
import { toast } from './toast.js';
import { connect, currentWindow, send } from './transport.js';

const SPIN_MS = 100;
const TICK_MS = 1000;

const handlers = { open: null };

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

function onSelect(name) {
  selectSession(name);
  render();
}

function onOpen(agent) {
  onSelect(agent.session);
}

function renderTabs() {
  $('tabs').replaceChildren(
    h('button', { class: 'tab active', type: 'button' }, svg(ICON_OVERVIEW), 'Overview'),
  );
}

function render() {
  if (!state.overview) return;
  renderTabs();
  renderMascot();
  renderSidebar(onSelect, onOpen);
  renderHead();
  renderBoard(handlers);
  renderComposer();
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
      render();
      break;
    case 'usage':
      state.usage = message.entries;
      state.usageAt = Date.now();
      renderSidebar(onSelect, onOpen);
      break;
    case 'notice':
      toast(message.text, message.tone);
      break;
    case 'folders':
      showFolders(message);
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

function onShortcut(event) {
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

function initWindow() {
  const win = currentWindow();
  $('win-min').addEventListener('click', () => win.minimize());
  $('win-max').addEventListener('click', () => win.toggleMaximize());
  $('win-close').addEventListener('click', () => win.close());
  $('brand-mascot').replaceChildren(glyph('idle'));
}

function renderMascot() {
  const mascot = $('brand-mascot').firstElementChild;
  const mood = moodOf(state.overview ? state.overview.agents : []);
  if (mascot && mascot.dataset.mood !== mood) mascot.dataset.mood = mood;
}

function start() {
  initWindow();
  initComposer();
  $('recover').addEventListener('click', () => send({ type: 'recover', session: state.session }));
  document.addEventListener('keydown', onShortcut);
  window.addEventListener('orkai:error', (event) => toast(event.detail, 'error'));
  setInterval(() => {
    state.frame += 1;
    spinBoard();
    spinSidebar();
  }, SPIN_MS);
  setInterval(() => {
    if (!state.overview) return;
    tickBoard();
    renderSidebar(onSelect, onOpen);
  }, TICK_MS);
  connect(onMessage).catch((err) => toast(`could not reach orkai: ${err}`, 'error'));
  $('task').focus();
}

start();
