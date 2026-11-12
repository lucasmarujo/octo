import { normalizeHex } from './accent';
import type { ThemeChoice } from './model';
import { DEFAULT_THEME, THEME_CHOICES } from './model';
import { load, save } from './storage';
import { Store } from './store';

export type Layout = 'list' | 'board';

export type Settings = {
  theme: ThemeChoice;
  accent: string | null;
  layout: Layout;
  haptics: boolean;
  reduced: boolean;
};

const NAME = 'settings';
const DEFAULTS: Settings = { theme: DEFAULT_THEME, accent: null, layout: 'list', haptics: true, reduced: false };

export const settings = new Store<Settings>(DEFAULTS);

export function readSettings(saved: Partial<Settings> | null): Settings {
  const source = saved || {};
  return {
    theme: THEME_CHOICES.includes(source.theme as ThemeChoice) ? (source.theme as ThemeChoice) : DEFAULTS.theme,
    accent: normalizeHex(source.accent),
    layout: source.layout === 'board' ? 'board' : DEFAULTS.layout,
    haptics: source.haptics !== false,
    reduced: source.reduced === true,
  };
}

export async function loadSettings(): Promise<void> {
  settings.set(readSettings(await load<Partial<Settings>>(NAME)));
}

export function changeSettings(part: Partial<Settings>): void {
  settings.patch(part);
  save(NAME, settings.get()).catch(() => {});
}
