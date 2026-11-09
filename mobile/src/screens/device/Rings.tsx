import { useEffect, useRef } from 'react';
import { Animated, Pressable, StyleSheet, Text, View } from 'react-native';

import { label } from '../../core/agents';
import { useSecond } from '../../core/clock';
import model from '../../core/model';
import type { UsageEntry } from '../../core/protocol';
import { EASE_OUT } from '../../theme/motion';
import { useStyles } from '../../theme/styles';
import { useReduced } from '../../theme/ThemeContext';
import type { Theme } from '../../theme/tokens';
import { CAPS, mono, sans, tracking } from '../../theme/type';
import { RingArt } from '../../ui/RingArt';

type Props = {
  entries: UsageEntry[];
  now: number;
  onOpen: () => void;
};

type Value = {
  text: string;
  tone: 'muted' | 'red' | 'faint';
};

const RING = 56;
const RING_STROKE = 4;
const RING_MARK = 13;
const PULSE_MS = 1200;
const PULSE_PEAK = 0.3;
const PULSE_SCALE = 1.06;

export function ringValue(entry: UsageEntry, now: number): Value {
  if (entry.windows.length) {
    const top = entry.windows[0];
    if (top.used >= model.USAGE_ALERT && top.resetsAt) return { text: `${Math.round(top.used)}% · ${model.duration(top.resetsAt - now)}`, tone: 'red' };
    return {
      text: `${entry.windows
        .map(window => Math.round(window.used))
        .slice(0, 2)
        .join(' · ')}%`,
      tone: 'muted',
    };
  }
  if (entry.credits !== null) return { text: 'funds', tone: 'muted' };
  return { text: entry.problem || 'no limits', tone: 'faint' };
}

const styles = (t: Theme) =>
  StyleSheet.create({
    rings: { gap: 12, paddingTop: 4, paddingHorizontal: 18, paddingBottom: 6 },
    head: { flexDirection: 'row', justifyContent: 'space-between', alignItems: 'baseline' },
    caps: { ...mono(11), ...tracking(11, CAPS), color: t.textFaint },
    row: { flexDirection: 'row', justifyContent: 'space-between', gap: 6 },
    ring: { alignItems: 'center', gap: 6, width: 76 },
    name: { ...sans(13, 600, 16), color: t.text },
    value: { ...mono(11, 400, 14), color: t.textMuted },
    red: { color: t.lost },
    faint: { color: t.textFaint },
  });

function Ring({ entry, now }: { entry: UsageEntry; now: number }) {
  const s = useStyles(styles);
  const reduced = useReduced();
  const alert = entry.windows.some(window => window.used >= model.USAGE_ALERT);
  const pulse = useRef(new Animated.Value(0)).current;

  useEffect(() => {
    pulse.setValue(0);
    if (!alert || reduced) return undefined;
    const motion = Animated.timing(pulse, { toValue: 1, duration: PULSE_MS, easing: EASE_OUT, useNativeDriver: true });
    motion.start();
    return () => motion.stop();
  }, [pulse, alert, reduced]);

  const scale = pulse.interpolate({ inputRange: [0, PULSE_PEAK, 1], outputRange: [1, PULSE_SCALE, 1] });
  const value = ringValue(entry, now);
  return (
    <View style={s.ring}>
      <Animated.View style={{ transform: [{ scale }] }}>
        <RingArt entry={entry} size={RING} stroke={RING_STROKE} markSize={RING_MARK} />
      </Animated.View>
      <Text style={s.name}>{label(entry.kind)}</Text>
      <Text style={[s.value, value.tone === 'red' && s.red, value.tone === 'faint' && s.faint]} numberOfLines={1}>
        {value.text}
      </Text>
    </View>
  );
}

export function Rings({ entries, now, onOpen }: Props) {
  const s = useStyles(styles);
  useSecond();
  if (!entries.length) return null;
  return (
    <Pressable style={s.rings} onPress={onOpen} accessibilityRole="button" accessibilityLabel="plan usage">
      <View style={s.head}>
        <Text style={s.caps}>PLAN USAGE</Text>
        <Text style={s.caps}>details ›</Text>
      </View>
      <View style={s.row}>
        {entries.map(entry => (
          <Ring key={entry.kind} entry={entry} now={now} />
        ))}
      </View>
    </Pressable>
  );
}
