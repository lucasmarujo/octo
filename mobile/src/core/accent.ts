type Rgb = [red: number, green: number, blue: number];

type Accent = {
  ACCENT_DEFAULT: string;
  ACCENT_PRESETS: string[];
  HUE_MAX: number;
  normalizeHex(text: unknown): string | null;
  hexToRgb(hex: string): Rgb;
  rgbToHex(rgb: number[]): string;
  hueOf(hex: string): number;
  withHue(hex: string, hue: number): string;
  accentInk(hex: string): string;
  accentLink(hex: string, light: boolean): string;
};

const accent: Accent = require('../../../desktop/ui/js/accent.js');

export const { ACCENT_DEFAULT, ACCENT_PRESETS, HUE_MAX, normalizeHex, hexToRgb, rgbToHex, hueOf, withHue, accentInk, accentLink } = accent;
