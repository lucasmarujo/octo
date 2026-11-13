import { StyleSheet, Text, View } from 'react-native';

import { mix } from '../theme/color';
import { useStyles } from '../theme/styles';
import type { Theme } from '../theme/tokens';
import { mono, sans } from '../theme/type';
import { Press } from './Press';

export type SegmentTone = 'plain' | 'accent' | 'waiting';

export type SegmentItem<T extends string> = {
  value: T;
  text: string;
  count?: number;
  tone?: SegmentTone;
};

type Props<T extends string> = {
  items: SegmentItem<T>[];
  value: T;
  label: string;
  stretch?: boolean;
  onPick: (value: T) => void;
};

const INSET = 3;

const styles = (t: Theme) => {
  const tones: Record<SegmentTone, { bg: string; ink: string }> = {
    plain: { bg: t.surfaceRaised, ink: t.text },
    accent: { bg: t.accent, ink: t.accentInk },
    waiting: { bg: mix(t.waiting, 18), ink: t.waiting },
  };
  return {
    tones,
    sheet: StyleSheet.create({
      segmented: { flexDirection: 'row', padding: INSET, borderRadius: 12, backgroundColor: t.surface, borderWidth: 1, borderColor: t.border },
      slot: { flexShrink: 1 },
      stretch: { flex: 1 },
      button: { flexDirection: 'row', alignItems: 'center', justifyContent: 'center', gap: 5, paddingVertical: 6, paddingHorizontal: 10, borderRadius: 9 },
      text: { ...sans(14, 600), color: t.textFaint },
      textOn: sans(14, 700),
      badge: { paddingVertical: 1, paddingHorizontal: 6, borderRadius: 999, backgroundColor: t.waiting },
      count: { ...mono(11, 700), color: t.waitingInk },
    }),
  };
};

export function Segmented<T extends string>({ items, value, label, stretch = false, onPick }: Props<T>) {
  const s = useStyles(styles);
  return (
    <View style={s.sheet.segmented} accessibilityRole="tablist" accessibilityLabel={label}>
      {items.map(item => {
        const on = item.value === value;
        const tone = s.tones[item.tone || 'plain'];
        return (
          <View key={item.value} style={stretch ? s.sheet.stretch : s.sheet.slot}>
            <Press style={[s.sheet.button, on && { backgroundColor: tone.bg }]} role="tab" selected={on} onPress={() => onPick(item.value)}>
              <Text style={on ? [s.sheet.textOn, { color: tone.ink }] : s.sheet.text} numberOfLines={1}>
                {item.text}
              </Text>
              {item.count ? (
                <View style={s.sheet.badge}>
                  <Text style={s.sheet.count}>{String(item.count)}</Text>
                </View>
              ) : null}
            </Press>
          </View>
        );
      })}
    </View>
  );
}
