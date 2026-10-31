import { h } from './dom.js';
import { palette } from './palette.js';
import { LINK_RECORD_OSC, decodeFrame, findLinks, keyRoute, parseLinkRecord, terminalTheme } from './termwire.js';

const DEFAULT = -1;
const TRUECOLOR = 256;
const BOLD = 1;
const DIM = 2;
const ITALIC = 4;
const UNDERLINE = 8;
const INVERSE = 16;
const INK = 'var(--term-bg)';
const TEXT = 'var(--term-fg)';
const FONT = '"JetBrains Mono", ui-monospace, monospace';
const FONT_SIZE = 13;
const LINE_HEIGHT = 1.05;
const SCROLLBACK = 5000;
const MIN_ROWS = 8;
const MIN_COLS = 20;
const FALLBACK_SIZE = { rows: 24, cols: 80 };
const RESIZE_MS = 60;
const REOPEN_MS = 2000;
const encoder = new TextEncoder();

function color(value) {
  if (value >= TRUECOLOR) {
    const rgb = value - TRUECOLOR;
    return `rgb(${(rgb >> 16) & 255},${(rgb >> 8) & 255},${rgb & 255})`;
  }
  return palette(value);
}

export function run([text, fg, bg, flags]) {
  if (fg === DEFAULT && bg === DEFAULT && !flags) return document.createTextNode(text);
  const node = document.createElement('span');
  node.textContent = text;
  let front = fg === DEFAULT ? null : color(fg);
  let back = bg === DEFAULT ? null : color(bg);
  if (flags & INVERSE) [front, back] = [back || INK, front || TEXT];
  if (front) node.style.color = front;
  if (back) node.style.backgroundColor = back;
  if (flags & BOLD) node.style.fontWeight = '700';
  if (flags & DIM) node.style.opacity = '0.55';
  if (flags & ITALIC) node.style.fontStyle = 'italic';
  if (flags & UNDERLINE) node.style.textDecoration = 'underline';
  return node;
}

export class Terminal {
  constructor(host, handlers) {
    this.host = host;
    this.handlers = handlers;
    this.screen = h('div', { class: 'term-screen' });
    this.live = h('button', { class: 'term-live', type: 'button', hidden: true, onclick: () => this.xterm.scrollToBottom() }, 'Back to live ↓');
    host.replaceChildren(this.screen, this.live);
    this.agent = null;
    this.size = null;
    this.ended = true;
    this.openedAt = 0;
    this.generation = 0;
    this.records = [];
    this.resizeTimer = 0;
    this.xterm = new window.Terminal({
      fontFamily: FONT,
      fontSize: FONT_SIZE,
      lineHeight: LINE_HEIGHT,
      cursorBlink: true,
      scrollback: SCROLLBACK,
      allowProposedApi: true,
      macOptionIsMeta: true,
      linkHandler: { activate: (event, uri) => this.follow(event, uri) },
    });
    this.fit = new window.FitAddon.FitAddon();
    this.xterm.loadAddon(this.fit);
    this.xterm.loadAddon(new window.Unicode11Addon.Unicode11Addon());
    this.xterm.unicode.activeVersion = '11';
    this.xterm.loadAddon(new window.WebLinksAddon.WebLinksAddon((event, uri) => this.follow(event, uri)));
    this.ready = document.fonts
      .load(`${FONT_SIZE}px ${FONT}`)
      .catch(() => [])
      .then(() => this.mount());
  }

  mount() {
    this.xterm.open(this.screen);
    this.host.dataset.renderer = 'dom';
    try {
      const webgl = new window.WebglAddon.WebglAddon();
      webgl.onContextLoss(() => {
        webgl.dispose();
        this.host.dataset.renderer = 'dom';
      });
      this.xterm.loadAddon(webgl);
      this.host.dataset.renderer = 'webgl';
    } catch {
      this.host.dataset.renderer = 'dom';
    }
    this.xterm.parser.registerOscHandler(LINK_RECORD_OSC, (payload) => {
      const record = parseLinkRecord(payload);
      if (record) this.records.push(record);
      return true;
    });
    this.xterm.registerLinkProvider({ provideLinks: (y, callback) => callback(this.titledLinks(y)) });
    this.xterm.attachCustomKeyEventHandler((event) => this.onKey(event));
    this.xterm.onData((data) => this.send(encoder.encode(data)));
    this.xterm.onBinary((data) => this.send(Uint8Array.from(data, (char) => char.charCodeAt(0) & 255)));
    this.xterm.onScroll(() => this.showLive());
    this.xterm.onWriteParsed(() => this.showLive());
    this.xterm.textarea.addEventListener('focus', () => this.host.classList.add('focused'));
    this.xterm.textarea.addEventListener('blur', () => this.host.classList.remove('focused'));
    new ResizeObserver(() => {
      clearTimeout(this.resizeTimer);
      this.resizeTimer = setTimeout(() => this.refit(), RESIZE_MS);
    }).observe(this.host);
    new MutationObserver(() => this.retheme()).observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme'] });
    this.retheme();
  }

  retheme() {
    const style = getComputedStyle(document.documentElement);
    this.xterm.options.theme = terminalTheme((name) => style.getPropertyValue(name).trim());
  }

  onKey(event) {
    if (event.type !== 'keydown') return true;
    if (this.handlers.shortcut(event)) {
      event.preventDefault();
      return false;
    }
    const route = keyRoute(event, this.xterm.hasSelection());
    if (route === 'copy') {
      this.handlers.copy(this.xterm.getSelection());
      this.xterm.clearSelection();
      event.preventDefault();
      return false;
    }
    return route !== 'paste';
  }

  follow(event, url) {
    if (event.ctrlKey || event.metaKey) this.handlers.link(url);
  }

  titledLinks(y) {
    const line = this.records.length ? this.xterm.buffer.active.getLine(y - 1) : null;
    if (!line) return undefined;
    const links = findLinks(line.translateToString(true), this.records).map(({ start, end, url }) => ({
      range: { start: { x: start + 1, y }, end: { x: end, y } },
      text: url,
      activate: (event) => this.follow(event, url),
    }));
    return links.length ? links : undefined;
  }

  showLive() {
    const buffer = this.xterm.buffer.active;
    this.live.hidden = buffer.viewportY >= buffer.baseY;
  }

  measure() {
    const dims = this.fit.proposeDimensions();
    if (!dims || !Number.isFinite(dims.rows) || !Number.isFinite(dims.cols)) return FALLBACK_SIZE;
    return { rows: Math.max(MIN_ROWS, dims.rows), cols: Math.max(MIN_COLS, dims.cols) };
  }

  async open(agent) {
    await this.ready;
    this.generation += 1;
    const generation = this.generation;
    this.agent = agent;
    this.ended = false;
    this.openedAt = Date.now();
    this.records = [];
    this.xterm.reset();
    this.size = this.measure();
    this.xterm.resize(this.size.cols, this.size.rows);
    await this.handlers.open(agent, this.size, (payload) => {
      if (generation === this.generation) this.receive(payload);
    });
  }

  receive(payload) {
    const frame = decodeFrame(payload);
    if (!frame) return;
    if (frame.kind === 'output') {
      this.xterm.write(frame.data);
    } else if (frame.kind === 'snapshot') {
      this.records = [];
      this.xterm.reset();
      if (frame.rows !== this.xterm.rows || frame.cols !== this.xterm.cols) this.xterm.resize(frame.cols, frame.rows);
      this.xterm.write(frame.data);
    } else {
      this.ended = true;
    }
  }

  stale(agent) {
    return this.agent === agent && this.ended && Date.now() - this.openedAt > REOPEN_MS;
  }

  send(bytes) {
    if (this.agent !== null && !this.ended) this.handlers.input(bytes);
  }

  refit() {
    if (this.agent === null || !this.screen.clientWidth) return;
    const next = this.measure();
    if (this.size && next.rows === this.size.rows && next.cols === this.size.cols) return;
    this.size = next;
    this.xterm.resize(next.cols, next.rows);
    if (!this.ended) this.handlers.resize(next);
  }

  focus() {
    this.ready.then(() => this.xterm.focus());
  }

  close() {
    this.generation += 1;
    if (this.agent !== null) this.handlers.close();
    this.agent = null;
    this.ended = true;
    this.live.hidden = true;
  }
}
