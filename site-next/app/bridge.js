import { FALLBACK_VERSION, HOLD_MS, INTRO, MAIN_SESSION, PREFERENCES, SCRIPT, createHub } from './demo.js';

const PAGE = 'octo-page';
const DEMO = 'octo-demo';
const UI_PAGE = 'index.html';
const UI_NOTES = 'js/notes.js';
const LATEST_VERSION = /version: '([\d.]+)'/;
const VIEW_KEY = 'orkai.view';
const SESSION_KEY = 'orkai.session';
const OCTO_VIEW_KEY = 'octo.orchestratorView';
const TYPE_MS = 38;
const SUBMIT_MS = 450;
const POLL_MS = 250;

const reduced = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
const framed = window.parent !== window;
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const $ = (id) => document.getElementById(id);

let visible = !framed;
let heldUntil = 0;

function memoryStorage(seed) {
  const items = new Map(Object.entries(seed));
  return {
    get length() {
      return items.size;
    },
    key: (index) => [...items.keys()][index] ?? null,
    getItem: (key) => (items.has(key) ? items.get(key) : null),
    setItem: (key, value) => {
      items.set(key, String(value));
    },
    removeItem: (key) => {
      items.delete(key);
    },
    clear: () => items.clear(),
  };
}

function installStorage() {
  try {
    Object.defineProperty(window, 'localStorage', { configurable: true, value: memoryStorage(PREFERENCES) });
  } catch {
    for (const [key, value] of Object.entries(PREFERENCES)) window.localStorage.setItem(key, value);
  }
}

function guardFocus() {
  const focus = HTMLElement.prototype.focus;
  HTMLElement.prototype.focus = function guarded() {
    if (!framed || Date.now() < heldUntil) focus.call(this, { preventScroll: true });
  };
}

class Channel {
  constructor() {
    this.onmessage = null;
  }
}

function installTauri(hub) {
  const done = async () => {};
  const unlisten = async () => () => {};
  const appWindow = { isDecorated: async () => false, minimize: done, toggleMaximize: done, close: done, setTitle: done };
  window.__TAURI__ = {
    core: { invoke: hub.invoke, Channel },
    event: { listen: unlisten },
    webview: { getCurrentWebview: () => ({ onDragDropEvent: unlisten }) },
    window: { getCurrentWindow: () => appWindow },
  };
}

async function read(file) {
  const response = await fetch(file);
  if (!response.ok) throw new Error(`could not read ${file}`);
  return response.text();
}

async function latestVersion() {
  const match = LATEST_VERSION.exec(await read(UI_NOTES).catch(() => ''));
  return match ? match[1] : FALLBACK_VERSION;
}

function load(asset) {
  return new Promise((resolve, reject) => {
    const node = document.createElement(asset.localName);
    for (const { name, value } of asset.attributes) node.setAttribute(name, value);
    node.addEventListener('load', resolve);
    node.addEventListener('error', () => reject(new Error(`could not load ${asset.getAttribute('src') || asset.getAttribute('href')}`)));
    document.head.append(node);
  });
}

async function mount() {
  const page = new DOMParser().parseFromString(await read(UI_PAGE), 'text/html');
  const assets = [...page.head.querySelectorAll('link[rel="stylesheet"], script[src]')];
  document.title = page.title;
  for (const asset of assets.filter((entry) => entry.type !== 'module')) await load(asset);
  document.body.replaceChildren(...[...page.body.childNodes].map((node) => document.adoptNode(node)));
  for (const asset of assets.filter((entry) => entry.type === 'module')) await import(new URL(asset.getAttribute('src'), document.baseURI).href);
}

function narrate(line) {
  if (framed) window.parent.postMessage({ source: DEMO, say: line.say, mood: line.mood }, window.location.origin);
}

function listen() {
  window.addEventListener('message', (event) => {
    if (event.origin !== window.location.origin || event.source !== window.parent) return;
    if (!event.data || event.data.source !== PAGE) return;
    visible = event.data.visible === true;
  });
  for (const type of ['pointerdown', 'keydown']) {
    window.addEventListener(type, (event) => {
      if (event.isTrusted) heldUntil = Date.now() + HOLD_MS;
    }, true);
  }
  if (framed) window.parent.postMessage({ source: DEMO, ready: true }, window.location.origin);
}

const idle = () => visible && Date.now() >= heldUntil && !$('dialog').open;

async function whenIdle() {
  while (!idle()) await sleep(POLL_MS);
}

async function rest(ms) {
  await sleep(ms);
  await whenIdle();
}

function showOverview() {
  if (!$('overview').hidden) return;
  const tab = document.querySelector('#tabs [data-tab="overview"]');
  if (tab) tab.click();
  else document.dispatchEvent(new KeyboardEvent('keydown', { key: ']', ctrlKey: true, bubbles: true }));
}

function showSession() {
  if (window.localStorage.getItem(SESSION_KEY) === MAIN_SESSION) return;
  const row = document.querySelector(`#sessions [data-session="${MAIN_SESSION}"]`);
  if (row) row.click();
}

function showView(view) {
  showOverview();
  showSession();
  if (window.localStorage.getItem(VIEW_KEY) !== view) document.querySelector(`#views [data-view="${view}"]`).click();
}

function showChat() {
  if (!$('octo-view').hidden && window.localStorage.getItem(OCTO_VIEW_KEY) === 'chat') return;
  const entry = document.querySelector('#octo-nav button');
  if (entry) entry.click();
}

function show(view) {
  if (view === 'chat') showChat();
  else showView(view);
}

async function typeTask(text) {
  const task = $('task');
  for (let index = 1; index <= text.length; index += 1) {
    task.value = text.slice(0, index);
    task.dispatchEvent(new Event('input', { bubbles: true }));
    await sleep(TYPE_MS);
  }
  await sleep(SUBMIT_MS);
  $('composer').requestSubmit();
}

async function perform(hub, step) {
  if (step.type === 'launch') await typeTask(step.task);
  else hub.play(step);
}

async function rendered() {
  while (!document.querySelector('#sessions [data-session]')) await sleep(POLL_MS);
}

async function tour(hub) {
  for (;;) {
    for (const beat of SCRIPT) {
      await whenIdle();
      show(beat.view);
      narrate(beat);
      if (beat.step) await perform(hub, beat.step);
      await rest(beat.wait);
    }
    hub.reset();
  }
}

async function boot() {
  installStorage();
  guardFocus();
  const hub = createHub({ version: await latestVersion() });
  installTauri(hub);
  listen();
  await mount();
  await rendered();
  if (reduced) narrate(INTRO);
  else tour(hub);
}

boot();
