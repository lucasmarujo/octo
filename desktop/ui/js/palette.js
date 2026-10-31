export const BASE16 = [
  '#2c2c3a', '#f04e5c', '#50d782', '#f0d65a', '#6e96ff', '#ec80dc', '#5ad2d2', '#dedee4',
  '#6e6e7a', '#ff7d9d', '#96e2aa', '#f0de96', '#89bafa', '#e8aaec', '#8ce2d6', '#ffffff',
];

export const ansi = (index) => `var(--ansi-${index})`;

export function palette(index) {
  if (index < 16) return ansi(index);
  if (index < 232) {
    const value = index - 16;
    const level = (step) => (step === 0 ? 0 : 55 + step * 40);
    return `rgb(${level(Math.floor(value / 36))},${level(Math.floor(value / 6) % 6)},${level(value % 6)})`;
  }
  const gray = 8 + (index - 232) * 10;
  return `rgb(${gray},${gray},${gray})`;
}

const SGR = /\x1b\[([0-9;]*)m/g;
const OTHER_ESCAPES = /\x1b\[[0-9;?]*[A-Za-ln-z]|\x1b\][^\x07]*\x07/g;

function apply(style, codes) {
  for (let i = 0; i < codes.length; i += 1) {
    const code = codes[i];
    if (code === 0) Object.assign(style, { color: null, bold: false, dim: false });
    else if (code === 1) style.bold = true;
    else if (code === 2) style.dim = true;
    else if (code === 22) Object.assign(style, { bold: false, dim: false });
    else if (code >= 30 && code <= 37) style.color = ansi(code - 30);
    else if (code >= 90 && code <= 97) style.color = ansi(code - 90 + 8);
    else if (code === 39) style.color = null;
    else if (code === 38 && codes[i + 1] === 5) {
      style.color = palette(codes[i + 2] || 0);
      i += 2;
    } else if (code === 38 && codes[i + 1] === 2) {
      style.color = `rgb(${codes[i + 2] || 0},${codes[i + 3] || 0},${codes[i + 4] || 0})`;
      i += 4;
    } else if (code === 48 && (codes[i + 1] === 5 || codes[i + 1] === 2)) {
      i += codes[i + 1] === 5 ? 2 : 4;
    }
  }
}

export function sgrRuns(line, style = { color: null, bold: false, dim: false }) {
  const runs = [];
  const text = line.replace(OTHER_ESCAPES, '');
  let at = 0;
  const push = (chunk) => {
    if (chunk) runs.push({ text: chunk, ...style });
  };
  for (const match of text.matchAll(SGR)) {
    push(text.slice(at, match.index));
    apply(style, match[1] === '' ? [0] : match[1].split(';').map(Number));
    at = match.index + match[0].length;
  }
  push(text.slice(at));
  return runs;
}
