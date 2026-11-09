import React, { useEffect, useMemo, useRef, useState } from 'react';
import type { LayoutChangeEvent } from 'react-native';
import { Animated, BackHandler, PanResponder, Pressable, ScrollView, StyleSheet, Text, useWindowDimensions, View } from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';

import { Store, useStore } from '../core/store';
import { BASE_MS, EASE_OUT, EASE_SPRING, lasting, SHEET_MS } from '../theme/motion';
import { FILL, useStyles } from '../theme/styles';
import { useReduced } from '../theme/ThemeContext';
import type { Theme } from '../theme/tokens';
import { mono, sans, TIGHT, tracking } from '../theme/type';
import { Round } from './atoms';
import { useKeyboard } from './keyboard';

export type SheetSpec = {
  title: string;
  sub?: string;
  lead?: React.ReactNode;
  tall?: boolean;
  agentHead?: boolean;
  content: React.ReactNode;
};

type Shown = SheetSpec & { id: number };

const DRAG_CLOSE = 90;
const HEIGHT_SHARE = 0.88;
const SIDE = 14;
let opened = 0;

export const sheets = new Store<Shown | null>(null);

export function openSheet(spec: SheetSpec): void {
  opened += 1;
  sheets.set({ ...spec, id: opened });
}

export function closeSheet(): void {
  sheets.set(null);
}

const styles = (t: Theme) =>
  StyleSheet.create({
    scrim: { ...FILL, backgroundColor: t.scrim },
    card: {
      width: '100%',
      borderTopLeftRadius: 30,
      borderTopRightRadius: 30,
      backgroundColor: t.sheet,
      borderTopWidth: 1,
      borderColor: t.borderStrong,
      paddingHorizontal: SIDE,
      boxShadow: [{ offsetX: 0, offsetY: -20, blurRadius: 60, color: 'rgba(0, 0, 0, 0.4)' }],
    },
    grab: { alignItems: 'center', paddingTop: 10, paddingBottom: 6 },
    grabBar: { width: 38, height: 5, borderRadius: 3, backgroundColor: t.borderStrong },
    head: { flexDirection: 'row', alignItems: 'center', gap: 12, paddingTop: 4, paddingHorizontal: 4, paddingBottom: 14 },
    titles: { flex: 1, minWidth: 0, gap: 2 },
    title: { ...sans(24, 800, 28), ...tracking(24, TIGHT), color: t.text },
    agentTitle: { ...mono(17, 700, 22), color: t.text },
    sub: { ...sans(14), color: t.textMuted },
    agentSub: { ...mono(12), color: t.textMuted },
    scroll: { flexShrink: 1, flexGrow: 1, minHeight: 0 },
    scrollContent: { gap: 14 },
    foot: { paddingTop: 4 },
  });

export function SheetScroll({ children }: { children: React.ReactNode }) {
  const s = useStyles(styles);
  return (
    <ScrollView style={s.scroll} contentContainerStyle={s.scrollContent} keyboardShouldPersistTaps="handled" showsVerticalScrollIndicator={false}>
      {children}
    </ScrollView>
  );
}

export function SheetFoot({ children }: { children: React.ReactNode }) {
  const s = useStyles(styles);
  return <View style={s.foot}>{children}</View>;
}

export function SheetHost() {
  const spec = useStore(sheets);
  const s = useStyles(styles);
  const reduced = useReduced();
  const insets = useSafeAreaInsets();
  const window = useWindowDimensions();
  const keyboard = useKeyboard();
  const [shown, setShown] = useState<Shown | null>(null);
  const [height, setHeight] = useState(window.height);
  const open = useRef(new Animated.Value(0)).current;
  const drag = useRef(new Animated.Value(0)).current;

  useEffect(() => {
    if (spec) {
      setShown(spec);
      drag.setValue(0);
      Animated.timing(open, { toValue: 1, duration: lasting(SHEET_MS, reduced), easing: EASE_SPRING, useNativeDriver: true }).start();
      return;
    }
    Animated.timing(open, { toValue: 0, duration: lasting(SHEET_MS, reduced), easing: EASE_OUT, useNativeDriver: true }).start(({ finished }) => {
      if (finished) setShown(null);
    });
  }, [spec, open, drag, reduced]);

  useEffect(() => {
    if (!spec) return undefined;
    const back = BackHandler.addEventListener('hardwareBackPress', () => {
      closeSheet();
      return true;
    });
    return () => back.remove();
  }, [spec]);

  const responder = useMemo(
    () =>
      PanResponder.create({
        onMoveShouldSetPanResponder: (_, gesture) => Math.abs(gesture.dy) > 4 && Math.abs(gesture.dy) > Math.abs(gesture.dx),
        onPanResponderMove: (_, gesture) => drag.setValue(Math.max(0, gesture.dy)),
        onPanResponderRelease: (_, gesture) => {
          if (gesture.dy > DRAG_CLOSE) {
            closeSheet();
            return;
          }
          Animated.timing(drag, { toValue: 0, duration: lasting(BASE_MS, reduced), easing: EASE_SPRING, useNativeDriver: true }).start();
        },
        onPanResponderTerminate: () => drag.setValue(0),
      }),
    [drag, reduced],
  );

  if (!shown) return null;
  const room = window.height - keyboard;
  const limit = room * HEIGHT_SHARE;
  const translateY = Animated.add(open.interpolate({ inputRange: [0, 1], outputRange: [height, 0] }), drag);
  const scrim = open.interpolate({ inputRange: [0, 1], outputRange: [0, 1], extrapolate: 'clamp' });
  const measure = (event: LayoutChangeEvent) => setHeight(event.nativeEvent.layout.height);

  return (
    <View style={[StyleSheet.absoluteFill, { justifyContent: 'flex-end', paddingBottom: keyboard }]} pointerEvents={spec ? 'auto' : 'none'}>
      <Animated.View style={[s.scrim, { opacity: scrim }]}>
        <Pressable style={StyleSheet.absoluteFill} onPress={closeSheet} accessibilityLabel="close" />
      </Animated.View>
      <Animated.View
        onLayout={measure}
        accessibilityViewIsModal
        style={[s.card, { maxHeight: limit, paddingBottom: (keyboard ? 0 : insets.bottom) + SIDE, transform: [{ translateY }] }, shown.tall && { height: limit }]}>
        <View {...responder.panHandlers}>
          <View style={s.grab}>
            <View style={s.grabBar} />
          </View>
          <View style={s.head}>
            {shown.lead}
            <View style={s.titles}>
              <Text style={shown.agentHead ? s.agentTitle : s.title}>{shown.title}</Text>
              {shown.sub ? <Text style={shown.agentHead ? s.agentSub : s.sub}>{shown.sub}</Text> : null}
            </View>
            <Round icon="close" label="close" small onPress={closeSheet} />
          </View>
        </View>
        <View key={shown.id} style={{ flexShrink: 1, flexGrow: shown.tall ? 1 : 0, minHeight: 0 }}>
          {shown.content}
        </View>
      </Animated.View>
    </View>
  );
}
