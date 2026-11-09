import type { ThemeChoice } from './model';
import model from './model';
import { load, save } from './storage';
import { Store } from './store';

export type Layout = 'list' | 'board';

export type Settings = {
  theme: ThemeChoice;
  layout: Layout;
  haptics: boolean;
  reduced: boolean;
};

const NAME = 'settings';
const DEFAULTS: Settings = { theme: 'dark', layout: 'list', haptics: true, reduced: false };

export const settings = new Store<Settings>(DEFAULTS);

export function readSettings(saved: Partial<Settings> | null): Settings {
  const source = saved || {};
  return {
    theme: model.THEMES.includes(source.theme as ThemeChoice) ? (source.theme as ThemeChoice) : DEFAULTS.theme,
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
