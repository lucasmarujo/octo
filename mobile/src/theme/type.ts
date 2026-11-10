import type { TextStyle } from 'react-native';
import { Platform } from 'react-native';

export const SANS = 'Barlow';
export const MONO = 'JetBrains Mono';
export const TERM = Platform.select({ ios: 'ui-monospace', default: 'monospace' });
export const ANDROID_STAND_INS: Readonly<Record<string, string>> = { '⏵': '▸', '⏸': '‖', '⏺': '●' };
const TERM_STAND_INS = Platform.OS === 'android' ? ANDROID_STAND_INS : null;
const STAND_IN = new RegExp(`[${Object.keys(ANDROID_STAND_INS).join('')}]`, 'g');
export const LEADING = 1.4;
export const TIGHT = -0.02;
export const CAPS = 0.08;

export type SansWeight = 400 | 600 | 700 | 800;
export type MonoWeight = 400 | 700;

const SANS_FACE: Record<SansWeight, TextStyle['fontWeight']> = { 400: '400', 600: '600', 700: '800', 800: '800' };
const MONO_FACE: Record<MonoWeight, TextStyle['fontWeight']> = { 400: '400', 700: '700' };

export function sans(size: number, weight: SansWeight = 400, line: number = size * LEADING): TextStyle {
  return { fontFamily: SANS, fontWeight: SANS_FACE[weight], fontSize: size, lineHeight: line, includeFontPadding: false };
}

export function mono(size: number, weight: MonoWeight = 400, line: number = size * LEADING): TextStyle {
  return { fontFamily: MONO, fontWeight: MONO_FACE[weight], fontSize: size, lineHeight: line, includeFontPadding: false };
}

export function termText(text: string, table: Readonly<Record<string, string>> | null = TERM_STAND_INS): string {
  return table ? text.replace(STAND_IN, glyph => table[glyph] ?? glyph) : text;
}

export function tracking(size: number, em: number): TextStyle {
  return { letterSpacing: size * em };
}
