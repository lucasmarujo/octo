import { h } from './dom.js';
import { palette } from './palette.js';

const DEFAULT = -1;
const TRUECOLOR = 256;
const BOLD = 1;
const DIM = 2;
const ITALIC = 4;
const UNDERLINE = 8;
const INVERSE = 16;
const PAD = 16;
const MIN_ROWS = 8;
const MIN_COLS = 20;
const RESIZE_MS = 120;
const WHEEL_STEP = 40;
const INK = '#08080b';
const TEXT = '#ececf2';
const APP_KEYS = new Set(['Shift', 'Control', 'Alt', 'Meta', 'CapsLock', 'NumLock', 'ScrollLock', 'Process', 'Dead', 'Unidentified']);

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
    this.rows = h('div', { class: 'term-rows' });
    this.cursor = h('div', { class: 'term-cursor', hidden: true });
    this.sink = h('textarea', { class: 'term-sink', 'aria-label': 'Type into the agent', spellcheck: 'false', autocapitalize: 'off' });
    this.live = h('button', { class: 'term-live', type: 'button', hidden: true, onclick: () => handlers.scroll(0) }, 'Back to live ↓');
    this.probe = h('span', { class: 'term-probe' }, 'MMMMMMMMMM');
    host.replaceChildren(this.rows, this.cursor, this.sink, this.live, this.probe);
    this.screen = null;
    this.agent = null;
    this.wheel = 0;
    this.resizeTimer = 0;
    this.size = null;
    this.bind();
  }

  bind() {
    this.host.addEventListener('mouseup', () => {
      if (!window.getSelection().toString()) this.focus();
    });
    this.sink.addEventListener('keydown', (event) => this.onKey(event));
    this.sink.addEventListener('paste', (event) => {
      event.preventDefault();
      const text = event.clipboardData.getData('text/plain');
      if (text) this.handlers.paste(text);
    });
    this.sink.addEventListener('compositionend', (event) => {
      if (event.data) this.handlers.paste(event.data);
      this.sink.value = '';
    });
    this.sink.addEventListener('focus', () => this.host.classList.add('focused'));
    this.sink.addEventListener('blur', () => this.host.classList.remove('focused'));
    this.host.addEventListener('wheel', (event) => {
      event.preventDefault();
      this.wheel += event.deltaY;
      const lines = Math.trunc(this.wheel / WHEEL_STEP);
      if (!lines) return;
      this.wheel -= lines * WHEEL_STEP;
      this.handlers.scroll(-lines * 3);
    }, { passive: false });
    new ResizeObserver(() => {
      clearTimeout(this.resizeTimer);
      this.resizeTimer = setTimeout(() => this.refit(), RESIZE_MS);
    }).observe(this.host);
  }

  focus() {
    this.sink.focus({ preventScroll: true });
  }

  onKey(event) {
    if (event.isComposing || APP_KEYS.has(event.key)) return;
    if (this.handlers.shortcut(event)) {
      event.preventDefault();
      return;
    }
    const selection = window.getSelection().toString();
    if (event.ctrlKey && !event.altKey && !event.shiftKey && event.key.toLowerCase() === 'c' && selection) {
      event.preventDefault();
      this.handlers.copy(selection);
      window.getSelection().removeAllRanges();
      return;
    }
    if ((event.ctrlKey && !event.altKey && event.key.toLowerCase() === 'v') || (event.shiftKey && event.key === 'Insert')) {
      return;
    }
    event.preventDefault();
    this.handlers.key({ key: event.key, ctrl: event.ctrlKey, alt: event.altKey, shift: event.shiftKey });
  }

  metrics() {
    const box = this.probe.getBoundingClientRect();
    const style = getComputedStyle(this.rows);
    return { width: box.width / 10 || 8, height: parseFloat(style.lineHeight) || 18 };
  }

  measure() {
    const { width, height } = this.metrics();
    const rows = Math.max(MIN_ROWS, Math.floor((this.host.clientHeight - PAD * 2) / height));
    const cols = Math.max(MIN_COLS, Math.floor((this.host.clientWidth - PAD * 2) / width));
    return { rows, cols };
  }

  open(agent) {
    this.agent = agent;
    this.screen = null;
    this.rows.replaceChildren();
    this.cursor.hidden = true;
    this.live.hidden = true;
    this.size = this.measure();
    return this.size;
  }

  close() {
    this.agent = null;
    this.screen = null;
    this.rows.replaceChildren();
    this.cursor.hidden = true;
  }

  refit() {
    if (this.agent === null || !this.host.clientWidth) return;
    const next = this.measure();
    if (this.size && next.rows === this.size.rows && next.cols === this.size.cols) return;
    this.size = next;
    this.handlers.resize(next);
  }

  apply(message) {
    if (message.agent !== this.agent) return;
    if (message.full || !this.screen || this.rows.children.length !== message.rows) {
      this.rows.replaceChildren(...Array.from({ length: message.rows }, () => h('div', { class: 'term-row' })));
    }
    this.screen = { rows: message.rows, cols: message.cols, cursor: message.cursor, scroll: message.scroll };
    for (const [index, runs] of message.lines) {
      const row = this.rows.children[index];
      if (row) row.replaceChildren(...runs.map(run));
    }
    this.placeCursor();
    this.live.hidden = message.scroll === 0;
  }

  placeCursor() {
    const screen = this.screen;
    if (!screen || !screen.cursor || screen.scroll !== 0) {
      this.cursor.hidden = true;
      return;
    }
    const [row, col] = screen.cursor;
    const { width, height } = this.metrics();
    this.cursor.hidden = false;
    this.cursor.style.width = `${width}px`;
    this.cursor.style.height = `${height}px`;
    this.cursor.style.transform = `translate(${PAD + col * width}px, ${PAD + row * height}px)`;
  }
}
