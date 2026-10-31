export const OUTPUT = 0;
export const SNAPSHOT = 1;
export const ENDED = 2;
export const LINK_RECORD_OSC = 7502;

const ANSI = ['black', 'red', 'green', 'yellow', 'blue', 'magenta', 'cyan', 'white'];
const SELECTION_ALPHA = '59';
const utf8 = new TextDecoder();

function bytesOf(payload) {
  if (payload instanceof Uint8Array) return payload;
  if (payload instanceof ArrayBuffer) return new Uint8Array(payload);
  if (Array.isArray(payload)) return Uint8Array.from(payload);
  return null;
}

export function decodeFrame(payload) {
  const bytes = bytesOf(payload);
  if (!bytes || !bytes.length) return null;
  if (bytes[0] === OUTPUT) return { kind: 'output', data: bytes.subarray(1) };
  if (bytes[0] === ENDED) return { kind: 'ended' };
  if (bytes[0] === SNAPSHOT && bytes.length >= 5) {
    return { kind: 'snapshot', rows: bytes[1] | (bytes[2] << 8), cols: bytes[3] | (bytes[4] << 8), data: bytes.subarray(5) };
  }
  return null;
}

function unbase64(text) {
  return utf8.decode(Uint8Array.from(atob(text), (char) => char.charCodeAt(0)));
}

export function parseLinkRecord(payload) {
  const [url, text] = payload.split(';');
  if (!url || !text) return null;
  try {
    return { url: unbase64(url), text: unbase64(text) };
  } catch {
    return null;
  }
}

export function findLinks(line, records) {
  const found = [];
  for (const { url, text } of records) {
    for (let at = line.indexOf(text); at !== -1; at = line.indexOf(text, at + text.length)) {
      found.push({ start: at, end: at + text.length, url });
    }
  }
  return found;
}

export function keyRoute(event, hasSelection) {
  const ctrl = event.ctrlKey && !event.altKey && !event.metaKey;
  const key = event.key.toLowerCase();
  if (ctrl && !event.shiftKey && key === 'c' && hasSelection) return 'copy';
  if ((ctrl && key === 'v') || (event.shiftKey && event.key === 'Insert')) return 'paste';
  return 'terminal';
}

export function terminalTheme(read) {
  const theme = {
    background: read('--term-bg'),
    foreground: read('--term-fg'),
    cursor: read('--term-fg'),
    cursorAccent: read('--term-bg'),
    selectionBackground: `${read('--color-accent')}${SELECTION_ALPHA}`,
  };
  ANSI.forEach((name, index) => {
    theme[name] = read(`--ansi-${index}`);
    theme[`bright${name[0].toUpperCase()}${name.slice(1)}`] = read(`--ansi-${index + ANSI.length}`);
  });
  return theme;
}
