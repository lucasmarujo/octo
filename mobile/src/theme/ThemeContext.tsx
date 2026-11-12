import React, { createContext, useContext, useMemo } from 'react';
import { useColorScheme } from 'react-native';

import { DEFAULT_THEME, resolveTheme } from '../core/model';
import { settings } from '../core/settings';
import { useSelect } from '../core/store';
import type { Theme } from './tokens';
import { THEMES, tinted } from './tokens';

const Context = createContext<Theme>(THEMES[DEFAULT_THEME]);

export function ThemeProvider({ children }: { children: React.ReactNode }) {
  const choice = useSelect(settings, state => state.theme);
  const accent = useSelect(settings, state => state.accent);
  const scheme = useColorScheme();
  const theme = useMemo(() => tinted(THEMES[resolveTheme(choice, scheme === 'light')], accent), [choice, accent, scheme]);
  return <Context.Provider value={theme}>{children}</Context.Provider>;
}

export function useTheme(): Theme {
  return useContext(Context);
}

export function useReduced(): boolean {
  return useSelect(settings, state => state.reduced);
}
