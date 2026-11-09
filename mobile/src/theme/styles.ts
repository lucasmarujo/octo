import type { ThemeName } from '../core/model';
import { useTheme } from './ThemeContext';
import type { Theme } from './tokens';

type Maker<T> = (theme: Theme) => T;

export const FILL = { position: 'absolute', top: 0, right: 0, bottom: 0, left: 0 } as const;

const cache = new Map<Maker<unknown>, Map<ThemeName, unknown>>();

export function useStyles<T>(make: Maker<T>): T {
  const theme = useTheme();
  let made = cache.get(make as Maker<unknown>);
  if (!made) {
    made = new Map();
    cache.set(make as Maker<unknown>, made);
  }
  if (!made.has(theme.name)) made.set(theme.name, make(theme));
  return made.get(theme.name) as T;
}
