import { ENDED, OUTPUT, SNAPSHOT } from './termwire.js';

const ESC = '\x1b';
const DEFAULT = -1;
const TRUECOLOR = 256;
const STYLES = [[1, '1'], [2, '2'], [4, '3'], [8, '4'], [16, '7']];
const RESET = `${ESC}[0m`;
const QUIET = `${ESC}[?25l${ESC}[?7l`;
const WHEEL_LINES = 3;
const encoder = new TextEncoder();
const decoder = new TextDecoder();

function paint(code, value) {
  if (value === DEFAULT) return [];
  if (value < TRUECOLOR) return [`${code};5;${value}`];
  const rgb = value - TRUECOLOR;
  return [`${code};2;${(rgb >> 16) & 255};${(rgb >> 8) & 255};${rgb & 255}`];
}

function styled([text, fg, bg, flags]) {
  const codes = ['0', ...STYLES.filter(([flag]) => flags & flag).map(([, code]) => code), ...paint('38', fg), ...paint('48', bg)];
  return `${ESC}[${codes.join(';')}m${text}`;
}

export function screenAnsi(screen) {
  const parts = [QUIET, screen.full ? `${RESET}${ESC}[2J` : ''];
  for (const [row, runs] of screen.lines) parts.push(`${ESC}[${row + 1};1H${RESET}${ESC}[2K`, ...runs.map(styled));
  parts.push(RESET);
  if (screen.cursor) parts.push(`${ESC}[${screen.cursor[0] + 1};${screen.cursor[1] + 1}H${ESC}[?25h`);
  return parts.join('');
}

export function screenFrame(screen) {
  const data = encoder.encode(screenAnsi(screen));
  const head = screen.full ? [SNAPSHOT, screen.rows & 255, screen.rows >> 8, screen.cols & 255, screen.cols >> 8] : [OUTPUT];
  const frame = new Uint8Array(head.length + data.length);
  frame.set(head);
  frame.set(data, head.length);
  return frame;
}

export function wheelLines(deltaY) {
  return deltaY ? -Math.sign(deltaY) * WHEEL_LINES : 0;
}

export class RemoteView {
  constructor(send, running) {
    this.send = send;
    this.running = running;
    this.shown = null;
    this.next = 0;
  }

  mine(session) {
    return this.shown !== null && this.shown.session === session;
  }

  open(agent, size, onFrame) {
    this.next += 1;
    this.shown = { session: this.next, agent, onFrame, painted: false, ended: false };
    this.send({ type: 'open', agent, rows: size.rows, cols: size.cols });
    return Promise.resolve(this.next);
  }

  input(session, bytes) {
    if (this.mine(session)) this.send({ type: 'keys', data: decoder.decode(bytes) });
  }

  paste(session, text) {
    if (this.mine(session)) this.send({ type: 'paste', text });
  }

  resize(session, size) {
    if (this.mine(session)) this.send({ type: 'size', rows: size.rows, cols: size.cols });
  }

  wheel(session, deltaY) {
    const lines = wheelLines(deltaY);
    if (lines && this.mine(session)) this.send({ type: 'scroll', lines });
  }

  close(session) {
    if (!this.mine(session)) return;
    this.shown = null;
    this.send({ type: 'close' });
  }

  onScreen(screen) {
    const shown = this.shown;
    if (!shown || shown.agent !== screen.agent) return;
    shown.painted = true;
    shown.onFrame(screenFrame(screen));
    this.settle();
  }

  settle() {
    const shown = this.shown;
    if (!shown || shown.ended || !shown.painted || this.running(shown.agent)) return;
    shown.ended = true;
    shown.onFrame(Uint8Array.of(ENDED));
  }

  handlers(base) {
    return {
      ...base,
      open: (agent, size, onFrame) => this.open(agent, size, onFrame),
      input: (session, bytes) => this.input(session, bytes),
      paste: (session, text) => this.paste(session, text),
      resize: (session, size) => this.resize(session, size),
      wheel: (session, deltaY) => this.wheel(session, deltaY),
      close: (session) => this.close(session),
    };
  }
}
