import type { BoxShadowValue } from 'react-native';

import { accentInk, accentLink } from '../core/accent';
import type { ThemeName } from '../core/model';
import type { AgentKind } from '../core/protocol';

export type Kinds = Record<AgentKind | 'other', string>;

export type Theme = {
  name: ThemeName;
  light: boolean;
  bg: string;
  surface: string;
  surfaceRaised: string;
  border: string;
  borderStrong: string;
  text: string;
  textMuted: string;
  textFaint: string;
  accent: string;
  accentInk: string;
  waiting: string;
  waitingInk: string;
  done: string;
  doneInk: string;
  lost: string;
  lostInk: string;
  link: string;
  scrim: string;
  shadow: BoxShadowValue;
  track: string;
  termBg: string;
  termFg: string;
  sheet: string;
  kind: Kinds;
  ansi: string[];
};

const old: Theme = {
  name: 'old',
  light: false,
  bg: '#0e0e13',
  surface: '#16161d',
  surfaceRaised: '#1e1e28',
  border: '#2a2a36',
  borderStrong: '#3a3a48',
  text: '#ececf2',
  textMuted: '#9a9aa8',
  textFaint: '#6a6a78',
  accent: '#e85eb6',
  accentInk: '#1a0a10',
  waiting: '#f0d65a',
  waitingInk: '#1a1608',
  done: '#50d782',
  doneInk: '#0b2416',
  lost: '#f04e5c',
  lostInk: '#1a0a10',
  link: '#6e96ff',
  scrim: 'rgba(5, 5, 8, 0.62)',
  shadow: { offsetX: 0, offsetY: 18, blurRadius: 40, color: 'rgba(0, 0, 0, 0.55)' },
  track: 'rgba(236, 236, 242, 0.08)',
  termBg: '#08080b',
  termFg: '#ececf2',
  sheet: '#1e1e28',
  kind: {
    claude: '#de805c',
    codex: '#78afff',
    kilo: '#ece26e',
    opencode: '#fab283',
    antigravity: '#9696ff',
    grok: '#e8e8f0',
    qwen: '#ba8cff',
    kimi: '#ec80dc',
    pi: '#aadc5a',
    terminal: '#5ad2d2',
    other: '#9a9aa8',
  },
  ansi: ['#2c2c3a', '#f04e5c', '#50d782', '#f0d65a', '#6e96ff', '#ec80dc', '#5ad2d2', '#dedee4', '#6e6e7a', '#ff7d9d', '#96e2aa', '#f0de96', '#89bafa', '#e8aaec', '#8ce2d6', '#ffffff'],
};

const octo: Theme = {
  ...old,
  name: 'octo',
  bg: '#000000',
  surface: '#0a0b0c',
  surfaceRaised: '#141718',
  border: '#1a1d1f',
  borderStrong: '#23282a',
  accent: '#35fddc',
  accentInk: '#04201c',
  link: '#7dfee8',
  scrim: 'rgba(0, 0, 0, 0.62)',
  termBg: '#000000',
  sheet: '#141718',
};

const gray: Theme = {
  ...octo,
  name: 'gray',
  bg: '#282828',
  surface: '#303030',
  surfaceRaised: '#3a3a3a',
  border: '#444444',
  borderStrong: '#585858',
  text: '#ececec',
  textMuted: '#b0b0b0',
  textFaint: '#8a8a8a',
  lost: '#ff6673',
  termBg: '#1e1e1e',
  termFg: '#ececec',
  sheet: '#3a3a3a',
  track: 'rgba(236, 236, 236, 0.1)',
  scrim: old.scrim,
  kind: { ...octo.kind, grok: '#e8e8e8', other: '#b0b0b0' },
  ansi: octo.ansi.map((color, index) => ({ 0: '#3a3a3a', 1: '#ff6673', 7: '#dedede', 8: '#7a7a7a' }[index] || color)),
};

const light: Theme = {
  ...octo,
  name: 'light',
  light: true,
  accent: '#12b8a0',
  bg: '#ffffff',
  surface: '#f6f6f9',
  surfaceRaised: '#ffffff',
  border: '#e0e0e8',
  borderStrong: '#c8c8d4',
  text: '#16161d',
  textMuted: '#55556a',
  textFaint: '#8a8a9a',
  waiting: '#a88600',
  waitingInk: '#ffffff',
  done: '#1a9a55',
  doneInk: '#ffffff',
  lost: '#d93545',
  lostInk: '#ffffff',
  link: '#2f62d8',
  scrim: 'rgba(22, 22, 29, 0.32)',
  shadow: { offsetX: 0, offsetY: 10, blurRadius: 30, color: 'rgba(22, 22, 29, 0.12)' },
  track: 'rgba(22, 22, 29, 0.08)',
  termBg: '#fbfbfd',
  termFg: '#16161d',
  sheet: '#ffffff',
  kind: {
    claude: '#c4613a',
    codex: '#2f6fd6',
    kilo: '#8f7f00',
    opencode: '#c4702f',
    antigravity: '#5050d8',
    grok: '#3a3a48',
    qwen: '#7d45cc',
    kimi: '#b8409f',
    pi: '#4f7d12',
    terminal: '#0f8585',
    other: '#6a6a78',
  },
  ansi: ['#16161d', '#c8303f', '#1b8a4a', '#8f6f00', '#2f5fd6', '#a8389c', '#0f8585', '#55556a', '#8a8a9a', '#dc4258', '#26a35c', '#a88600', '#4a7bea', '#c04eb4', '#169a9f', '#2c2c3a'],
};

export const THEMES: Record<ThemeName, Theme> = { octo, gray, light, old };
const FIXED: ThemeName = 'old';

export function tinted(theme: Theme, accent: string | null): Theme {
  if (!accent || theme.name === FIXED) return theme;
  return { ...theme, accent, accentInk: accentInk(accent), link: accentLink(accent, theme.light) };
}
