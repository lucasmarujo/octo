import { useEffect, useMemo, useRef, useState } from 'react';
import { Animated, Easing, PanResponder, StyleSheet, Text, View } from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';

import type { TileTone } from '../core/agents';
import type { Tone } from '../core/protocol';
import { Store, useStore } from '../core/store';
import { mix } from '../theme/color';
import { EASE_OUT, EASE_SPRING, FAST_MS, lasting } from '../theme/motion';
import { useStyles } from '../theme/styles';
import { useReduced, useTheme } from '../theme/ThemeContext';
import type { Theme } from '../theme/tokens';
import { mono, sans } from '../theme/type';
import { Tile } from './atoms';
import { Press } from './Press';

export type ToastMark = {
  text: string;
  tile: TileTone;
};

export type ToastAction = {
  label: string;
  run: () => void;
};

export type ToastOptions = {
  title?: string;
  mark?: ToastMark;
  action?: ToastAction;
};

type Shown = ToastOptions & {
  id: number;
  text: string;
  tone: Tone;
  ms: number;
};

const TOAST_MS = 3500;
const TOAST_ERROR_MS = 6000;
const IN_MS = 300;
const SWIPE_AWAY = -30;
const LIFT = -24;
const SMALL = 0.96;
const MARKS: Record<Tone, ToastMark> = {
  warn: { text: '‖', tile: 'needs' },
  success: { text: '✓', tile: 'done' },
  error: { text: '!', tile: 'lostFill' },
  info: { text: 'i', tile: 'idle' },
};
let shownCount = 0;

export const toasts = new Store<Shown | null>(null);

export function toast(text: string, tone: Tone = 'info', options: ToastOptions = {}): void {
  shownCount += 1;
  toasts.set({ ...options, id: shownCount, text, tone, ms: tone === 'error' ? TOAST_ERROR_MS : TOAST_MS });
}

export function hideToast(): void {
  toasts.set(null);
}

const styles = (t: Theme) =>
  StyleSheet.create({
    toast: {
      position: 'absolute',
      left: 12,
      right: 12,
      borderRadius: 20,
      backgroundColor: t.surfaceRaised,
      borderWidth: 1,
      borderColor: t.borderStrong,
      boxShadow: [t.shadow],
      overflow: 'hidden',
    },
    body: { flexDirection: 'row', alignItems: 'center', gap: 12, padding: 12 },
    words: { flex: 1, minWidth: 0 },
    title: { ...mono(14, 700, 20), color: t.text },
    text: { ...sans(15, 400, 20), color: t.text },
    under: { ...sans(14, 400, 20), color: t.textMuted },
    action: { paddingVertical: 8, paddingHorizontal: 12, borderRadius: 10, backgroundColor: t.surface, flexShrink: 0 },
    actionText: { ...sans(14, 700), color: t.text },
    time: { height: 2, transformOrigin: 'left' },
  });

export function ToastHost() {
  const spec = useStore(toasts);
  const t = useTheme();
  const s = useStyles(styles);
  const reduced = useReduced();
  const insets = useSafeAreaInsets();
  const [shown, setShown] = useState<Shown | null>(null);
  const enter = useRef(new Animated.Value(0)).current;
  const time = useRef(new Animated.Value(1)).current;

  useEffect(() => {
    if (!spec) {
      Animated.timing(enter, { toValue: 0, duration: lasting(FAST_MS, reduced), easing: EASE_OUT, useNativeDriver: true }).start(({ finished }) => {
        if (finished) setShown(null);
      });
      return undefined;
    }
    setShown(spec);
    enter.setValue(0);
    time.setValue(1);
    Animated.timing(enter, { toValue: 1, duration: lasting(IN_MS, reduced), easing: EASE_SPRING, useNativeDriver: true }).start();
    Animated.timing(time, { toValue: 0, duration: spec.ms, easing: Easing.linear, useNativeDriver: true }).start();
    const timer = setTimeout(() => {
      if (toasts.get() === spec) hideToast();
    }, spec.ms);
    return () => clearTimeout(timer);
  }, [spec, enter, time, reduced]);

  const responder = useMemo(
    () =>
      PanResponder.create({
        onMoveShouldSetPanResponder: (_, gesture) => gesture.dy < -6,
        onPanResponderRelease: (_, gesture) => {
          if (gesture.dy < SWIPE_AWAY) hideToast();
        },
      }),
    [],
  );

  if (!shown) return null;
  const edge = { warn: mix(t.waiting, 40), error: mix(t.lost, 45), success: t.borderStrong, info: t.borderStrong }[shown.tone];
  const bar = { warn: t.waiting, success: t.done, error: t.lost, info: t.link }[shown.tone];
  const tint = { warn: t.waiting, success: t.done, error: null, info: null }[shown.tone];
  const mark = shown.mark || MARKS[shown.tone];
  const action = shown.action;
  const translateY = enter.interpolate({ inputRange: [0, 1], outputRange: [LIFT, 0] });
  const scale = enter.interpolate({ inputRange: [0, 1], outputRange: [SMALL, 1] });
  const opacity = enter.interpolate({ inputRange: [0, 1], outputRange: [0, 1], extrapolate: 'clamp' });

  return (
    <Animated.View {...responder.panHandlers} accessibilityRole="alert" style={[s.toast, { top: insets.top + 8, borderColor: edge, opacity, transform: [{ translateY }, { scale }] }]}>
      <View style={s.body}>
        <Tile tone={mark.tile} text={mark.text} />
        <View style={s.words}>
          {shown.title ? <Text style={s.title}>{shown.title}</Text> : null}
          <Text style={shown.title ? s.under : s.text}>{shown.text}</Text>
        </View>
        {action ? (
          <Press
            style={[s.action, tint ? { backgroundColor: mix(tint, 14) } : null]}
            onPress={() => {
              hideToast();
              action.run();
            }}>
            <Text style={[s.actionText, tint ? { color: tint } : null]}>{action.label}</Text>
          </Press>
        ) : null}
      </View>
      <Animated.View style={[s.time, { backgroundColor: bar, transform: [{ scaleX: time }] }]} />
    </Animated.View>
  );
}
