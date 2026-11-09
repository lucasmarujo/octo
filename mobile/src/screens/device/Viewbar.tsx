import { useEffect, useRef, useState } from 'react';
import type { LayoutChangeEvent } from 'react-native';
import { Animated, StyleSheet, Text, View } from 'react-native';

import { waiting } from '../../core/agents';
import type { AgentView } from '../../core/protocol';
import type { Layout } from '../../core/settings';
import { BASE_MS, EASE_OUT, lasting } from '../../theme/motion';
import { useStyles } from '../../theme/styles';
import { useReduced, useTheme } from '../../theme/ThemeContext';
import type { Theme } from '../../theme/tokens';
import { mono, sans } from '../../theme/type';
import { Icon } from '../../ui/Icon';
import { Press } from '../../ui/Press';

type Props = {
  layout: Layout;
  agents: AgentView[];
  onLayout: (layout: Layout) => void;
};

type Slot = {
  x: number;
  width: number;
};

const MODES: { mode: Layout; text: string }[] = [
  { mode: 'list', text: 'List' },
  { mode: 'board', text: 'Board' },
];
const INSET = 3;

const styles = (t: Theme) =>
  StyleSheet.create({
    viewbar: { flexDirection: 'row', alignItems: 'center', justifyContent: 'space-between', paddingTop: 14, paddingHorizontal: 18, paddingBottom: 10 },
    segmented: { flexDirection: 'row', padding: INSET, borderRadius: 12, backgroundColor: t.surface, borderWidth: 1, borderColor: t.border },
    button: { flexDirection: 'row', alignItems: 'center', gap: 6, paddingVertical: 6, paddingHorizontal: 12, borderRadius: 9 },
    text: { ...sans(14, 600), color: t.textFaint },
    textOn: { ...sans(14, 700), color: t.text },
    thumb: { position: 'absolute', top: INSET, bottom: INSET, left: 0, borderRadius: 9, backgroundColor: t.surfaceRaised, boxShadow: [{ offsetX: 0, offsetY: 1, blurRadius: 3, color: 'rgba(0, 0, 0, 0.3)' }] },
    counts: { flexDirection: 'row', alignItems: 'center', gap: 10 },
    count: { flexDirection: 'row', alignItems: 'center', gap: 4 },
    mark: mono(12, 700),
    number: { ...mono(12), color: t.textMuted },
  });

export function Viewbar({ layout, agents, onLayout }: Props) {
  const t = useTheme();
  const s = useStyles(styles);
  const reduced = useReduced();
  const [slots, setSlots] = useState<Partial<Record<Layout, Slot>>>({});
  const x = useRef(new Animated.Value(0)).current;
  const width = useRef(new Animated.Value(0)).current;
  const placed = useRef(false);
  const slot = slots[layout];

  useEffect(() => {
    if (!slot) return;
    if (!placed.current) {
      placed.current = true;
      x.setValue(slot.x);
      width.setValue(slot.width);
      return;
    }
    const duration = lasting(BASE_MS, reduced);
    Animated.parallel([
      Animated.timing(x, { toValue: slot.x, duration, easing: EASE_OUT, useNativeDriver: false }),
      Animated.timing(width, { toValue: slot.width, duration, easing: EASE_OUT, useNativeDriver: false }),
    ]).start();
  }, [slot, x, width, reduced]);

  const measure = (mode: Layout) => (event: LayoutChangeEvent) => {
    const { x: left, width: wide } = event.nativeEvent.layout;
    setSlots(current => (current[mode] && current[mode]!.x === left && current[mode]!.width === wide ? current : { ...current, [mode]: { x: left, width: wide } }));
  };

  const count = (test: (agent: AgentView) => boolean) => agents.filter(test).length;
  const parts = [
    { n: count(agent => agent.state === 'working' || agent.starting), mark: '⠹', color: t.waiting },
    { n: count(waiting), mark: '‖', color: t.waiting },
    { n: count(agent => agent.state === 'done'), mark: '✓', color: t.done },
    { n: count(agent => agent.state === 'lost'), mark: '↻', color: t.lost },
  ].filter(part => part.n);

  return (
    <View style={s.viewbar}>
      <View style={s.segmented} accessibilityRole="tablist">
        <Animated.View style={[s.thumb, { width, transform: [{ translateX: x }] }]} />
        {MODES.map(({ mode, text }) => (
          <View key={mode} onLayout={measure(mode)}>
            <Press style={s.button} role="tab" selected={mode === layout} onPress={() => onLayout(mode)}>
              <Icon name={mode} size={15} color={mode === layout ? t.text : t.textFaint} />
              <Text style={mode === layout ? s.textOn : s.text}>{text}</Text>
            </Press>
          </View>
        ))}
      </View>
      <View style={s.counts}>
        {parts.map(part => (
          <View key={part.mark} style={s.count}>
            <Text style={[s.mark, { color: part.color }]}>{part.mark}</Text>
            <Text style={s.number}>{String(part.n)}</Text>
          </View>
        ))}
      </View>
    </View>
  );
}
