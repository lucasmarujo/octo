import React, { useEffect, useMemo, useRef, useState } from 'react';
import type { LayoutChangeEvent, TextStyle } from 'react-native';
import { Animated, Easing, PanResponder, ScrollView, StyleSheet, Text, View } from 'react-native';

import { testMark } from '../../core/agents';
import type { AgentView, Run } from '../../core/protocol';
import { useStore } from '../../core/store';
import type { ScreenState, TermSize } from '../../core/workspace';
import { MIN_COLS, MIN_ROWS, TERM_FONT, TERM_LINE, TERM_MAX, TERM_MIN, TERM_PAD } from '../../core/workspace';
import { mix } from '../../theme/color';
import { useStyles } from '../../theme/styles';
import { useReduced, useTheme } from '../../theme/ThemeContext';
import type { Theme } from '../../theme/tokens';
import { mono, sans, TERM, termText } from '../../theme/type';
import { toneColor } from '../../ui/atoms';
import { Press } from '../../ui/Press';
import { Rise } from '../../ui/Rise';
import { useDevice } from './context';

const DEFAULT = -1;
const TRUECOLOR = 256;
const BOLD = 1;
const DIM = 2;
const ITALIC = 4;
const UNDERLINE = 8;
const INVERSE = 16;
const DIM_SHARE = 55;
const PROBE = 'MMMMMMMMMM';
const PROBE_FONT = 30;
const FALLBACK_RATIO = 0.6;
const FONT_STEP = 4;
const BLINK_MS = 1100;
const CURSOR_OPACITY = 0.55;
const DRAG_SLOP = 4;

function palette(t: Theme, index: number): string {
  if (index < 16) return t.ansi[index];
  if (index < 232) {
    const value = index - 16;
    const level = (step: number) => (step === 0 ? 0 : 55 + step * 40);
    return `rgb(${level(Math.floor(value / 36))},${level(Math.floor(value / 6) % 6)},${level(value % 6)})`;
  }
  const gray = 8 + (index - 232) * 10;
  return `rgb(${gray},${gray},${gray})`;
}

function colorOf(t: Theme, value: number): string {
  if (value >= TRUECOLOR) {
    const rgb = value - TRUECOLOR;
    return `rgb(${(rgb >> 16) & 255},${(rgb >> 8) & 255},${rgb & 255})`;
  }
  return palette(t, value);
}

function runStyle(t: Theme, fg: number, bg: number, flags: number): TextStyle | null {
  if (fg === DEFAULT && bg === DEFAULT && !flags) return null;
  let front = fg === DEFAULT ? null : colorOf(t, fg);
  let back = bg === DEFAULT ? null : colorOf(t, bg);
  if (flags & INVERSE) [front, back] = [back || t.termBg, front || t.termFg];
  if (flags & DIM) {
    front = mix(front || t.termFg, DIM_SHARE);
    back = back ? mix(back, DIM_SHARE) : null;
  }
  return {
    ...(front ? { color: front } : null),
    ...(back ? { backgroundColor: back } : null),
    ...(flags & BOLD ? { fontWeight: '700' } : null),
    ...(flags & ITALIC ? { fontStyle: 'italic' } : null),
    ...(flags & UNDERLINE ? { textDecorationLine: 'underline' } : null),
  };
}

const Row = React.memo(function Row({ runs, font, theme }: { runs: Run[]; font: number; theme: Theme }) {
  const height = font * TERM_LINE;
  return (
    <Text numberOfLines={1} ellipsizeMode="clip" style={{ fontFamily: TERM, fontSize: font, lineHeight: height, height, color: theme.termFg, includeFontPadding: false }}>
      {runs.map(([text, fg, bg, flags], index) => {
        const style = runStyle(theme, fg, bg, flags);
        const shown = termText(text);
        return style ? (
          <Text key={index} style={style}>
            {shown}
          </Text>
        ) : (
          shown
        );
      })}
    </Text>
  );
});

const styles = (t: Theme) =>
  StyleSheet.create({
    context: { flexGrow: 0, flexShrink: 0 },
    contextContent: { gap: 6, paddingHorizontal: 14, paddingBottom: 10 },
    chip: { flexDirection: 'row', alignItems: 'center', gap: 6, paddingVertical: 5, paddingHorizontal: 10, borderRadius: 999, backgroundColor: t.surface },
    chipText: { ...mono(12), color: t.textMuted },
    branch: { backgroundColor: mix(t.link, 12) },
    card: { flex: 1, minHeight: 0, marginHorizontal: 10, borderRadius: 20, backgroundColor: t.termBg, borderWidth: 1, borderColor: t.border, overflow: 'hidden' },
    screen: { flex: 1, minHeight: 0, overflow: 'hidden' },
    rows: { paddingTop: TERM_PAD, paddingHorizontal: TERM_PAD },
    cursor: { position: 'absolute', top: 0, left: 0, backgroundColor: t.termFg },
    live: { position: 'absolute', right: 12, bottom: 12 },
    liveButton: { paddingVertical: 6, paddingHorizontal: 14, borderRadius: 999, borderWidth: 1, borderColor: t.accent, backgroundColor: t.bg },
    liveText: { ...sans(14, 700), color: t.accent },
    probe: { position: 'absolute', opacity: 0, width: 4000, flexDirection: 'row' },
  });

export function Chips({ agent }: { agent: AgentView | null }) {
  const t = useTheme();
  const s = useStyles(styles);
  if (!agent) return null;
  const changes = agent.changes && agent.changes.files ? agent.changes : null;
  const test = testMark(agent.test);
  const subs = (agent.subagents || []).length;
  if (!agent.branch && !changes && !test && !subs) return null;
  return (
    <ScrollView horizontal showsHorizontalScrollIndicator={false} style={s.context} contentContainerStyle={s.contextContent}>
      {agent.branch ? (
        <View style={[s.chip, s.branch]}>
          <Text style={[s.chipText, { color: t.link }]}>{`⎇ ${agent.branch}`}</Text>
        </View>
      ) : null}
      {changes ? (
        <View style={s.chip}>
          <Text style={[s.chipText, { color: t.done }]}>{`+${changes.insertions}`}</Text>
          <Text style={[s.chipText, { color: t.lost }]}>{`−${changes.deletions}`}</Text>
        </View>
      ) : null}
      {test ? (
        <View style={s.chip}>
          <Text style={[s.chipText, { color: toneColor(t, test.tone) }]}>{test.text}</Text>
        </View>
      ) : null}
      {subs ? (
        <View style={s.chip}>
          <Text style={s.chipText}>{`↳ ${subs} subagent${subs === 1 ? '' : 's'}`}</Text>
        </View>
      ) : null}
    </ScrollView>
  );
}

function Cursor({ screen, font, ratio, shift }: { screen: ScreenState; font: number; ratio: number; shift: number }) {
  const s = useStyles(styles);
  const reduced = useReduced();
  const blink = useRef(new Animated.Value(0)).current;

  useEffect(() => {
    blink.setValue(0);
    if (reduced) return undefined;
    const motion = Animated.loop(Animated.timing(blink, { toValue: 1, duration: BLINK_MS, easing: Easing.linear, useNativeDriver: true }));
    motion.start();
    return () => motion.stop();
  }, [blink, reduced]);

  if (!screen.cursor || screen.scroll) return null;
  const [row, col] = screen.cursor;
  const opacity = blink.interpolate({ inputRange: [0, 0.5, 0.5, 1], outputRange: [CURSOR_OPACITY, CURSOR_OPACITY, 0, 0] });
  return (
    <Animated.View
      pointerEvents="none"
      style={[s.cursor, { width: font * ratio, height: font * TERM_LINE, opacity, transform: [{ translateX: TERM_PAD + col * font * ratio }, { translateY: TERM_PAD + row * font * TERM_LINE - shift }] }]}
    />
  );
}

export function Terminal() {
  const { workspace } = useDevice();
  const t = useTheme();
  const s = useStyles(styles);
  const screen = useStore(workspace.screen);
  const [ratio, setRatio] = useState(FALLBACK_RATIO);
  const [box, setBox] = useState<{ width: number; height: number } | null>(null);
  const tallest = useRef({ width: 0, height: 0 });
  const dragged = useRef(0);

  const measure = (event: LayoutChangeEvent) => {
    const { width, height } = event.nativeEvent.layout;
    setBox(current => (current && current.width === width && current.height === height ? current : { width, height }));
  };
  const probe = (event: LayoutChangeEvent) => {
    const width = event.nativeEvent.layout.width;
    if (width > 0) setRatio(width / (PROBE.length * PROBE_FONT));
  };

  const size: TermSize | null = useMemo(() => {
    if (!box) return null;
    const width = box.width - TERM_PAD * 2;
    const height = box.height - TERM_PAD;
    if (width !== tallest.current.width) tallest.current = { width, height: 0 };
    tallest.current.height = Math.max(tallest.current.height, height);
    return {
      rows: Math.max(MIN_ROWS, Math.floor(tallest.current.height / (TERM_FONT * TERM_LINE))),
      cols: Math.max(MIN_COLS, Math.floor(width / (TERM_FONT * ratio))),
    };
  }, [box, ratio]);

  useEffect(() => {
    if (size) workspace.fit(size);
  }, [workspace, size]);

  const font = useMemo(() => {
    if (!screen || !box) return TERM_FONT;
    const fit = (box.width - TERM_PAD * 2) / (screen.cols * ratio);
    return Math.max(TERM_MIN, Math.min(TERM_MAX, Math.floor(fit * FONT_STEP) / FONT_STEP));
  }, [screen, box, ratio]);

  const line = font * TERM_LINE;
  const responder = useMemo(
    () =>
      PanResponder.create({
        onMoveShouldSetPanResponderCapture: (_, gesture) => Math.abs(gesture.dy) > DRAG_SLOP && Math.abs(gesture.dy) > Math.abs(gesture.dx),
        onPanResponderGrant: () => {
          dragged.current = 0;
        },
        onPanResponderMove: (_, gesture) => {
          const lines = Math.trunc((gesture.dy - dragged.current) / line);
          if (!lines) return;
          dragged.current += lines * line;
          workspace.scroll(lines);
        },
      }),
    [workspace, line],
  );

  const rowsHeight = screen ? screen.rows.length * line : 0;
  const shift = box ? Math.max(0, Math.round(rowsHeight + TERM_PAD - box.height)) : 0;
  const wide = screen ? Math.ceil(screen.cols * font * ratio) + TERM_PAD * 2 : 0;

  return (
    <View style={s.card}>
      <View style={s.probe} pointerEvents="none">
        <Text onLayout={probe} style={{ fontFamily: TERM, fontSize: PROBE_FONT, includeFontPadding: false }}>
          {PROBE}
        </Text>
      </View>
      <View style={s.screen} onLayout={measure} accessibilityLabel="agent terminal" {...responder.panHandlers}>
        {screen ? (
          <ScrollView horizontal showsHorizontalScrollIndicator={false} bounces={false} contentContainerStyle={{ minWidth: '100%' }}>
            <View style={[s.rows, { width: wide, minWidth: '100%', transform: [{ translateY: -shift }] }]}>
              {screen.rows.map((runs, index) => (
                <Row key={index} runs={runs} font={font} theme={t} />
              ))}
            </View>
            <Cursor screen={screen} font={font} ratio={ratio} shift={shift} />
          </ScrollView>
        ) : null}
      </View>
      {screen && screen.scroll !== 0 ? (
        <Rise style={s.live}>
          <Press style={s.liveButton} onPress={() => workspace.live()}>
            <Text style={s.liveText}>↓ live</Text>
          </Press>
        </Rise>
      ) : null}
    </View>
  );
}
