import { useEffect, useRef, useState } from 'react';
import { Animated, Pressable, ScrollView, StyleSheet, Text, View } from 'react-native';

import { label } from '../../core/agents';
import type { AgentView } from '../../core/protocol';
import type { Approval as Asking } from '../../core/workspace';
import { mix } from '../../theme/color';
import { BASE_MS, EASE_OUT, EASE_SPRING, lasting } from '../../theme/motion';
import { useStyles } from '../../theme/styles';
import { useReduced } from '../../theme/ThemeContext';
import type { Theme } from '../../theme/tokens';
import { CAPS, mono, sans, tracking } from '../../theme/type';
import { Press } from '../../ui/Press';
import { useDevice } from './context';

const KEY_GROUPS: { keys: [text: string, data: string][]; warn?: boolean }[] = [
  {
    keys: [
      ['esc', '\x1b'],
      ['tab', '\t'],
      ['⇧tab', '\x1b[Z'],
    ],
  },
  {
    keys: [
      ['↑', '\x1b[A'],
      ['↓', '\x1b[B'],
      ['←', '\x1b[D'],
      ['→', '\x1b[C'],
      ['⏎', '\r'],
    ],
  },
  {
    keys: [
      ['1', '1'],
      ['2', '2'],
      ['3', '3'],
      ['y', 'y'],
      ['n', 'n'],
    ],
  },
  { keys: [['^C', '\x03']], warn: true },
];
const ENTER_FROM = 24;
const FOLD_TO = 16;
const FOLD_SCALE = 0.98;
const CONTEXT_MAX = 72;

const styles = (t: Theme) =>
  StyleSheet.create({
    approval: {
      gap: 12,
      marginTop: 10,
      marginHorizontal: 10,
      padding: 16,
      borderRadius: 24,
      backgroundColor: t.surfaceRaised,
      borderWidth: 1,
      borderColor: mix(t.waiting, 45),
      boxShadow: [
        { offsetX: 0, offsetY: -10, blurRadius: 40, color: 'rgba(0, 0, 0, 0.35)' },
        { offsetX: 0, offsetY: 0, blurRadius: 0, spreadDistance: 4, color: mix(t.waiting, 6) },
      ],
      flexShrink: 0,
    },
    eyebrow: { flexDirection: 'row', alignItems: 'center', gap: 8 },
    eyebrowText: { ...mono(11, 400, 14), ...tracking(11, CAPS), color: t.waiting },
    eyebrowDot: { width: 7, height: 7, borderRadius: 3.5, backgroundColor: t.waiting, boxShadow: [{ offsetX: 0, offsetY: 0, blurRadius: 0, spreadDistance: 4, color: mix(t.waiting, 20) }] },
    question: { ...sans(19, 800, 24), color: t.text },
    context: { paddingVertical: 10, paddingHorizontal: 12, borderRadius: 12, backgroundColor: t.termBg, maxHeight: CONTEXT_MAX, overflow: 'hidden' },
    contextText: { ...mono(12, 400, 17), color: t.textMuted },
    options: { gap: 8 },
    first: { height: 48, borderRadius: 14, backgroundColor: t.waiting, paddingHorizontal: 14, alignItems: 'center', justifyContent: 'center' },
    firstText: { ...sans(16, 700), color: t.waitingInk },
    rest: { flexDirection: 'row', gap: 8, flexWrap: 'wrap' },
    other: { flexGrow: 1, flexShrink: 1, flexBasis: '40%', minHeight: 44, borderRadius: 14, backgroundColor: t.surface, borderWidth: 1, borderColor: t.border, paddingVertical: 6, paddingHorizontal: 12, alignItems: 'center', justifyContent: 'center' },
    otherText: { ...sans(14, 600, 18), color: t.text, textAlign: 'center' },
    danger: { backgroundColor: mix(t.lost, 12), borderColor: 'transparent' },
    dangerText: { ...sans(14, 700, 18), color: t.lost, textAlign: 'center' },
    hint: { ...sans(13), color: t.textFaint, textAlign: 'center' },
    keys: { flexGrow: 0, flexShrink: 0 },
    keysContent: { gap: 6, paddingTop: 10, paddingHorizontal: 10, paddingBottom: 8 },
    group: { flexDirection: 'row', gap: 2, padding: 3, borderRadius: 12, backgroundColor: t.surface, borderWidth: 1, borderColor: t.border },
    groupWarn: { backgroundColor: mix(t.lost, 12), borderColor: mix(t.lost, 30) },
    key: { minWidth: 34, paddingVertical: 6, paddingHorizontal: 9, borderRadius: 9, alignItems: 'center' },
    keyHeld: { backgroundColor: t.surfaceRaised },
    keyText: { ...mono(13), color: t.text },
    keyTextHeld: { color: t.accent },
    keyWarn: { ...mono(13, 700), color: t.lost },
  });

export function Approval({ asking, agent }: { asking: Asking; agent: AgentView | null }) {
  const { workspace } = useDevice();
  const s = useStyles(styles);
  const reduced = useReduced();
  const enter = useRef(new Animated.Value(0)).current;
  const fold = useRef(new Animated.Value(0)).current;

  useEffect(() => {
    enter.setValue(0);
    fold.setValue(0);
    Animated.timing(enter, { toValue: 1, duration: lasting(BASE_MS, reduced), easing: EASE_SPRING, useNativeDriver: true }).start();
  }, [asking.sig, enter, fold, reduced]);

  useEffect(() => {
    if (asking.answered) Animated.timing(fold, { toValue: 1, duration: lasting(BASE_MS, reduced), easing: EASE_OUT, useNativeDriver: true }).start();
  }, [asking.answered, fold, reduced]);

  const [first, ...rest] = asking.asked.options;
  const opacity = Animated.multiply(enter.interpolate({ inputRange: [0, 1], outputRange: [0, 1], extrapolate: 'clamp' }), fold.interpolate({ inputRange: [0, 1], outputRange: [1, 0] }));
  const translateY = Animated.add(enter.interpolate({ inputRange: [0, 1], outputRange: [ENTER_FROM, 0] }), fold.interpolate({ inputRange: [0, 1], outputRange: [0, FOLD_TO] }));
  const scale = fold.interpolate({ inputRange: [0, 1], outputRange: [1, FOLD_SCALE] });
  const answer = (key: string) => workspace.answer(key, asking.sig);

  return (
    <Animated.View style={[s.approval, { opacity, transform: [{ translateY }, { scale }] }]}>
      <View style={s.eyebrow}>
        <View style={s.eyebrowDot} />
        <Text style={s.eyebrowText}>NEEDS YOU</Text>
      </View>
      <Text style={s.question}>{asking.asked.question || (agent && agent.activity) || 'Pick an answer'}</Text>
      {asking.asked.context.length ? (
        <View style={s.context}>
          <Text style={s.contextText}>{asking.asked.context.join('\n')}</Text>
        </View>
      ) : null}
      <View style={s.options}>
        <Press style={s.first} onPress={() => answer(first.key)}>
          <Text style={s.firstText} numberOfLines={1}>
            {first.label}
          </Text>
        </Press>
        {rest.length ? (
          <View style={s.rest}>
            {rest.map(choice => (
              <Press key={choice.key} style={[s.other, choice.danger && s.danger]} onPress={() => answer(choice.key)}>
                <Text style={choice.danger ? s.dangerText : s.otherText}>{choice.label}</Text>
              </Press>
            ))}
          </View>
        ) : null}
      </View>
      {agent ? <Text style={s.hint}>{`or type below to tell ${label(agent.kind)} what to do instead`}</Text> : null}
    </Animated.View>
  );
}

function Key({ text, data, warn }: { text: string; data: string; warn: boolean }) {
  const { workspace } = useDevice();
  const s = useStyles(styles);
  const [held, setHeld] = useState(false);
  return (
    <Pressable style={[s.key, held && s.keyHeld]} onPressIn={() => setHeld(true)} onPressOut={() => setHeld(false)} onPress={() => workspace.keys(data)} accessibilityRole="button" accessibilityLabel={text}>
      <Text style={[warn ? s.keyWarn : s.keyText, held && !warn && s.keyTextHeld]}>{text}</Text>
    </Pressable>
  );
}

export function Keys() {
  const s = useStyles(styles);
  return (
    <ScrollView horizontal showsHorizontalScrollIndicator={false} style={s.keys} contentContainerStyle={s.keysContent} keyboardShouldPersistTaps="always" accessibilityLabel="keys">
      {KEY_GROUPS.map((group, index) => (
        <View key={index} style={[s.group, group.warn && s.groupWarn]}>
          {group.keys.map(([text, data]) => (
            <Key key={text} text={text} data={data} warn={Boolean(group.warn)} />
          ))}
        </View>
      ))}
    </ScrollView>
  );
}
