import type { StyleProp, TextStyle } from 'react-native';
import { Text } from 'react-native';
import { SvgXml } from 'react-native-svg';

import model from '../core/model';
import type { AgentKind } from '../core/protocol';
import { useTheme } from '../theme/ThemeContext';

type Icons = {
  PROVIDER_ICONS: Partial<Record<AgentKind, string>>;
};

type Props = {
  kind: AgentKind | string;
  size: number;
  style: StyleProp<TextStyle>;
};

const { PROVIDER_ICONS }: Icons = require('../../../desktop/ui/js/icons.js');

export function providerXml(kind: AgentKind | string): string | null {
  return Object.hasOwn(PROVIDER_ICONS, kind) ? PROVIDER_ICONS[kind as AgentKind] || null : null;
}

export function ProviderMark({ kind, size, style }: Props) {
  const t = useTheme();
  const xml = providerXml(kind);
  if (!xml) return <Text style={style}>{model.kindMark(kind)}</Text>;
  return <SvgXml xml={xml} width={size} height={size} color={t.text} />;
}
