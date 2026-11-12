export const ACCENT_DEFAULT = '#35fddc';
export const ACCENT_PRESETS = ['#35fddc', '#5aa9ff', '#8b7bff', '#f472b6', '#ff8a4c', '#f0d65a', '#7ce38b'];
export const HUE_MAX = 360;

const INK_DARK = '#0b0c0d';
const INK_LIGHT = '#ffffff';
const CONTRAST_OFFSET = 0.05;
const LINK_TINT = 0.4;
const LINK_SHADE = 0.35;
const HUE_SATURATION_MIN = 0.55;
const HUE_LIGHTNESS_MIN = 0.45;
const HUE_LIGHTNESS_MAX = 0.7;
const CHANNEL_MAX = 255;

const clamp = (value, min, max) => Math.min(max, Math.max(min, value));

export function normalizeHex(text) {
  const raw = String(text ?? '').trim().replace(/^#/, '').toLowerCase();
  if (/^[0-9a-f]{3}$/.test(raw)) return `#${[...raw].map((digit) => digit + digit).join('')}`;
  return /^[0-9a-f]{6}$/.test(raw) ? `#${raw}` : null;
}

export function hexToRgb(hex) {
  return [1, 3, 5].map((at) => Number.parseInt(hex.slice(at, at + 2), 16));
}

export function rgbToHex(rgb) {
  return `#${rgb.map((channel) => clamp(Math.round(Number(channel) || 0), 0, CHANNEL_MAX).toString(16).padStart(2, '0')).join('')}`;
}

export function rgbToHsl([red, green, blue]) {
  const [r, g, b] = [red, green, blue].map((channel) => channel / CHANNEL_MAX);
  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  const lightness = (max + min) / 2;
  if (max === min) return [0, 0, lightness];
  const spread = max - min;
  const saturation = lightness > 0.5 ? spread / (2 - max - min) : spread / (max + min);
  const sector = max === r ? (g - b) / spread + (g < b ? 6 : 0) : max === g ? (b - r) / spread + 2 : (r - g) / spread + 4;
  return [sector * 60, saturation, lightness];
}

export function hslToRgb([hue, saturation, lightness]) {
  const chroma = (1 - Math.abs(2 * lightness - 1)) * saturation;
  const sector = (((hue % HUE_MAX) + HUE_MAX) % HUE_MAX) / 60;
  const second = chroma * (1 - Math.abs((sector % 2) - 1));
  const base = lightness - chroma / 2;
  const [r, g, b] = [[chroma, second, 0], [second, chroma, 0], [0, chroma, second], [0, second, chroma], [second, 0, chroma], [chroma, 0, second]][Math.min(5, Math.floor(sector))];
  return [r, g, b].map((channel) => Math.round((channel + base) * CHANNEL_MAX));
}

export function hueOf(hex) {
  return Math.round(rgbToHsl(hexToRgb(hex))[0]);
}

export function withHue(hex, hue) {
  const [, saturation, lightness] = rgbToHsl(hexToRgb(hex));
  return rgbToHex(hslToRgb([hue, Math.max(saturation, HUE_SATURATION_MIN), clamp(lightness, HUE_LIGHTNESS_MIN, HUE_LIGHTNESS_MAX)]));
}

export function luminance(hex) {
  const [r, g, b] = hexToRgb(hex).map((channel) => channel / CHANNEL_MAX).map((value) => (value <= 0.03928 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4));
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

function contrast(first, second) {
  const [high, low] = [luminance(first), luminance(second)].sort((a, b) => b - a);
  return (high + CONTRAST_OFFSET) / (low + CONTRAST_OFFSET);
}

export function accentInk(hex) {
  return contrast(hex, INK_DARK) >= contrast(hex, INK_LIGHT) ? INK_DARK : INK_LIGHT;
}

function blend(hex, toward, amount) {
  const from = hexToRgb(hex);
  const to = hexToRgb(toward);
  return rgbToHex(from.map((channel, index) => channel + (to[index] - channel) * amount));
}

export function accentLink(hex, light) {
  return light ? blend(hex, '#000000', LINK_SHADE) : blend(hex, '#ffffff', LINK_TINT);
}

export function accentTokens(hex, light) {
  return { '--color-accent': hex, '--color-accent-ink': accentInk(hex), '--color-link': accentLink(hex, light) };
}
