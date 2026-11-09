import React from 'react';
import { StyleSheet, Text, View } from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';

import type { Mood } from '../core/model';
import { useStyles } from '../theme/styles';
import type { Theme } from '../theme/tokens';
import { mono, sans, TIGHT, tracking } from '../theme/type';
import { Round } from './atoms';
import type { IconName } from './Icon';
import { Kai } from './Kai';

type Action = {
  icon: IconName;
  label: string;
  run: () => void;
};

type Props = {
  title: string;
  mood: Mood;
  compact?: boolean;
  onBack?: () => void;
  backLabel?: string;
  action?: Action;
  children?: React.ReactNode;
};

const MASCOT = 44;
const MASCOT_COMPACT = 38;

const styles = (t: Theme) =>
  StyleSheet.create({
    top: { flexDirection: 'row', alignItems: 'center', gap: 12, paddingHorizontal: 18, paddingBottom: 12, flexShrink: 0 },
    who: { flex: 1, minWidth: 0, gap: 2 },
    title: { ...sans(22, 800, 24), ...tracking(22, TIGHT), color: t.text },
    compactTitle: { ...mono(16, 700, 20), color: t.text },
    sub: { flexDirection: 'row', alignItems: 'center', gap: 6, minWidth: 0, overflow: 'hidden', height: 16 },
  });

export function Top({ title, mood, compact = false, onBack, backLabel = 'back', action, children }: Props) {
  const s = useStyles(styles);
  const insets = useSafeAreaInsets();
  return (
    <View style={[s.top, { paddingTop: insets.top + 8 }]}>
      {onBack ? <Round icon="back" label={backLabel} onPress={onBack} /> : null}
      <Kai mood={mood} size={compact ? MASCOT_COMPACT : MASCOT} />
      <View style={s.who}>
        <Text style={compact ? s.compactTitle : s.title} numberOfLines={1}>
          {title}
        </Text>
        <View style={s.sub}>{children}</View>
      </View>
      {action ? <Round icon={action.icon} label={action.label} onPress={action.run} /> : null}
    </View>
  );
}

export const subStyles = (t: Theme) =>
  StyleSheet.create({
    text: { ...mono(12, 400, 16), color: t.textFaint },
  });
