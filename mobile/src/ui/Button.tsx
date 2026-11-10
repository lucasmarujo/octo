import React from 'react';
import type { StyleProp, ViewStyle } from 'react-native';
import { StyleSheet, Text } from 'react-native';

import { useStyles } from '../theme/styles';
import type { Theme } from '../theme/tokens';
import { sans } from '../theme/type';
import { Press } from './Press';

type Kind = 'primary' | 'secondary' | 'ghost' | 'danger';

type Props = {
  kind: Kind;
  text?: string;
  big?: boolean;
  flat?: boolean;
  style?: StyleProp<ViewStyle>;
  onPress: () => void;
  children?: React.ReactNode;
};

const styles = (t: Theme) =>
  StyleSheet.create({
    button: { flexDirection: 'row', alignItems: 'center', justifyContent: 'center', gap: 8, borderRadius: 14, paddingVertical: 10, paddingHorizontal: 16 },
    big: { alignSelf: 'stretch', height: 54, borderRadius: 16, paddingVertical: 0 },
    primary: { backgroundColor: t.accent, boxShadow: [{ offsetX: 0, offsetY: 10, blurRadius: 26, color: 'rgba(232, 94, 182, 0.32)' }] },
    flat: { boxShadow: [] },
    secondary: { backgroundColor: t.surface, borderWidth: 1, borderColor: t.border },
    ghost: { backgroundColor: t.surfaceRaised, borderWidth: 1, borderColor: t.border },
    danger: { backgroundColor: t.lost },
    primaryText: { ...sans(16, 700), color: t.accentInk },
    dangerText: { ...sans(16, 700), color: t.lostInk },
    plainText: { ...sans(16, 600), color: t.text },
    bigText: { fontSize: 17, lineHeight: 17 * 1.4 },
  });

export function Button({ kind, text, big = false, flat = false, style, onPress, children }: Props) {
  const s = useStyles(styles);
  return (
    <Press style={[s.button, s[kind], big && s.big, flat && s.flat, style]} onPress={onPress}>
      {children || <Text style={[kind === 'primary' ? s.primaryText : kind === 'danger' ? s.dangerText : s.plainText, big && s.bigText]}>{text}</Text>}
    </Press>
  );
}
