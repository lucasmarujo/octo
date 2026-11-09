import React, { createContext, useContext, useMemo } from 'react';
import { useColorScheme } from 'react-native';

import model from '../core/model';
import { settings } from '../core/settings';
import { useSelect } from '../core/store';
import type { Theme } from './tokens';
import { THEMES } from './tokens';

const Context = createContext<Theme>(THEMES.dark);

export function ThemeProvider({ children }: { children: React.ReactNode }) {
  const choice = useSelect(settings, state => state.theme);
  const scheme = useColorScheme();
  const theme = useMemo(() => THEMES[model.resolveTheme(choice, scheme === 'light')], [choice, scheme]);
  return <Context.Provider value={theme}>{children}</Context.Provider>;
}

export function useTheme(): Theme {
  return useContext(Context);
}

export function useReduced(): boolean {
  return useSelect(settings, state => state.reduced);
}
