type Rgba = [red: number, green: number, blue: number, alpha: number];

const HEX = /^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/i;
const FUNCTION = /^rgba?\(([^)]+)\)$/i;
const TRANSPARENT: Rgba = [0, 0, 0, 0];
const cache = new Map<string, string>();

function parse(color: string): Rgba {
  const hex = HEX.exec(color);
  if (hex) return [parseInt(hex[1], 16), parseInt(hex[2], 16), parseInt(hex[3], 16), 1];
  const fn = FUNCTION.exec(color);
  if (fn) {
    const parts = fn[1].split(/[\s,/]+/).filter(Boolean).map(Number);
    return [parts[0], parts[1], parts[2], parts.length > 3 ? parts[3] : 1];
  }
  return TRANSPARENT;
}

function print([red, green, blue, alpha]: Rgba): string {
  const channel = (value: number) => Math.round(Math.min(255, Math.max(0, value)));
  if (alpha >= 1) return `rgb(${channel(red)}, ${channel(green)}, ${channel(blue)})`;
  return `rgba(${channel(red)}, ${channel(green)}, ${channel(blue)}, ${Number(alpha.toFixed(4))})`;
}

function mixed(first: Rgba, share: number, second: Rgba): Rgba {
  const alpha = first[3] * share + second[3] * (1 - share);
  if (alpha === 0) return TRANSPARENT;
  const channel = (index: number) => (first[index] * first[3] * share + second[index] * second[3] * (1 - share)) / alpha;
  return [channel(0), channel(1), channel(2), alpha];
}

export function mix(color: string, percent: number, base?: string): string {
  const key = `${color}|${percent}|${base || ''}`;
  const known = cache.get(key);
  if (known) return known;
  const result = print(mixed(parse(color), percent / 100, base ? parse(base) : TRANSPARENT));
  cache.set(key, result);
  return result;
}
