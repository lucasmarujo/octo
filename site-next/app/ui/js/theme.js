import { ACCENT_DEFAULT, accentTokens, normalizeHex } from './accent.js';
import { storage } from './dom.js';
import { paintKai } from './kai.js';

export const THEME_KEY = 'orkai.theme';
export const ACCENT_KEY = 'octo.accent';
export const KAI_KEY = 'octo.kai';
export const KAI_ON = 'on';
export const DEFAULT_THEME = 'octo';
export const OLD_THEME = 'old';
export const THEME_CHOICES = ['octo', 'gray', 'light', 'old', 'system'];

const LIGHT_THEME = 'light';
const LIGHT_ACCENT = '#12b8a0';

export function themeChoice() {
  const chosen = storage.get(THEME_KEY);
  return THEME_CHOICES.includes(chosen) ? chosen : DEFAULT_THEME;
}

export function accentChoice() {
  return normalizeHex(storage.get(ACCENT_KEY));
}

export function kaiChoice() {
  return storage.get(KAI_KEY) === KAI_ON;
}

export function shownTheme() {
  return document.documentElement.dataset.theme;
}

export function shownAccent() {
  return accentChoice() || (shownTheme() === LIGHT_THEME ? LIGHT_ACCENT : ACCENT_DEFAULT);
}

export function applyAccent() {
  const root = document.documentElement;
  if (!root.style) return;
  const accent = shownTheme() === OLD_THEME ? null : accentChoice();
  const tokens = accentTokens(accent || ACCENT_DEFAULT, shownTheme() === LIGHT_THEME);
  for (const [name, value] of Object.entries(tokens)) {
    if (accent) root.style.setProperty(name, value);
    else root.style.removeProperty(name);
  }
  paintKai(shownTheme() === OLD_THEME ? null : shownAccent());
}

export function applyTheme() {
  const choice = themeChoice();
  const system = window.matchMedia('(prefers-color-scheme: light)').matches ? LIGHT_THEME : DEFAULT_THEME;
  document.documentElement.dataset.theme = choice === 'system' ? system : choice;
  document.documentElement.toggleAttribute('data-kai', kaiChoice());
  applyAccent();
}
