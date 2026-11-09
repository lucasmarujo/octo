import { useEffect, useRef } from 'react';
import type { StyleProp, TextStyle, ViewStyle } from 'react-native';
import { Animated, StyleSheet, Text, View } from 'react-native';

import type { TextTone, TileTone } from '../core/agents';
import { useFrame } from '../core/clock';
import type { FlashTone, LinkPhase } from '../core/link';
import model from '../core/model';
import type { AgentKind } from '../core/protocol';
import { mix } from '../theme/color';
import { useStyles } from '../theme/styles';
import { useReduced, useTheme } from '../theme/ThemeContext';
import type { Theme } from '../theme/tokens';
import { mono } from '../theme/type';
import type { IconName } from './Icon';
import { Icon } from './Icon';
import { Press } from './Press';

export function toneColor(t: Theme, tone: TextTone): string {
  return { text: t.text, muted: t.textMuted, faint: t.textFaint, yellow: t.waiting, green: t.done, red: t.lost, link: t.link }[tone];
}

export function kindColor(t: Theme, kind: AgentKind | string): string {
  return t.kind[kind as AgentKind] || t.kind.other;
}

export function Spin({ style }: { style?: StyleProp<TextStyle> }) {
  const frame = useFrame();
  return <Text style={style}>{model.SPINNER[frame % model.SPINNER.length]}</Text>;
}

export function Glyph({ text, spin, style }: { text: string; spin: boolean; style?: StyleProp<TextStyle> }) {
  return spin ? <Spin style={style} /> : <Text style={style}>{text}</Text>;
}

type TileProps = {
  tone: TileTone;
  text: string;
  spin?: boolean;
  big?: boolean;
  flash?: FlashTone | null;
};

const tileStyles = (t: Theme) => {
  const tones: Record<TileTone, { box: ViewStyle; ink: string }> = {
    working: { box: { backgroundColor: mix(t.waiting, 12) }, ink: t.waiting },
    needs: { box: { backgroundColor: t.waiting }, ink: t.waitingInk },
    done: { box: { backgroundColor: mix(t.done, 12) }, ink: t.done },
    lost: { box: { backgroundColor: mix(t.lost, 14) }, ink: t.lost },
    lostFill: { box: { backgroundColor: t.lost }, ink: t.lostInk },
    terminal: { box: { backgroundColor: mix(t.kind.terminal, 10) }, ink: t.kind.terminal },
    idle: { box: { backgroundColor: t.surfaceRaised }, ink: t.textFaint },
    starting: { box: { borderWidth: 1, borderStyle: 'dashed', borderColor: t.borderStrong }, ink: t.textFaint },
  };
  const flashes: Record<FlashTone, ViewStyle> = {
    done: {
      boxShadow: [
        { offsetX: 0, offsetY: 0, blurRadius: 0, spreadDistance: 6, color: mix(t.done, 12) },
        { offsetX: 0, offsetY: 0, blurRadius: 24, color: mix(t.done, 35) },
      ],
    },
    needs: {
      boxShadow: [
        { offsetX: 0, offsetY: 0, blurRadius: 0, spreadDistance: 6, color: mix(t.waiting, 14) },
        { offsetX: 0, offsetY: 0, blurRadius: 24, color: mix(t.waiting, 35) },
      ],
    },
  };
  return {
    tones,
    flashes,
    sheet: StyleSheet.create({
      tile: { width: 36, height: 36, borderRadius: 11, alignItems: 'center', justifyContent: 'center', flexShrink: 0 },
      big: { width: 40, height: 40, borderRadius: 13 },
      text: mono(15, 700),
      bigText: mono(19, 700),
    }),
  };
};

export function Tile({ tone, text, spin = false, big = false, flash = null }: TileProps) {
  const s = useStyles(tileStyles);
  const look = s.tones[tone];
  return (
    <View style={[s.sheet.tile, big && s.sheet.big, look.box, flash && s.flashes[flash]]}>
      <Glyph text={text} spin={spin} style={[big ? s.sheet.bigText : s.sheet.text, { color: look.ink }]} />
    </View>
  );
}

type RoundProps = {
  icon: IconName;
  label: string;
  onPress: () => void;
  small?: boolean;
  size?: number;
  radius?: number;
};

const roundStyles = (t: Theme) =>
  StyleSheet.create({
    round: { width: 40, height: 40, borderRadius: 14, backgroundColor: t.surface, borderWidth: 1, borderColor: t.border, alignItems: 'center', justifyContent: 'center', flexShrink: 0 },
    small: { width: 32, height: 32, borderRadius: 999, borderWidth: 0 },
  });

export function Round({ icon, label, onPress, small = false, size, radius }: RoundProps) {
  const t = useTheme();
  const s = useStyles(roundStyles);
  const sized = size ? { width: size, height: size, borderRadius: radius } : null;
  return (
    <Press onPress={onPress} label={label} style={[s.round, small && s.small, sized]}>
      <Icon name={icon} size={small ? 14 : 20} color={small ? t.textMuted : t.text} stroke={small ? 2.4 : 2} />
    </Press>
  );
}

const DOT_TONE: Record<LinkPhase, 'waiting' | 'done' | 'lost'> = { connecting: 'waiting', live: 'done', offline: 'lost', unpaired: 'lost', replaced: 'lost', outdated: 'lost' };
const PULSE_MS = 1000;
const PULSE_LOW = 0.25;

export function Dot({ phase }: { phase: LinkPhase }) {
  const t = useTheme();
  const reduced = useReduced();
  const opacity = useRef(new Animated.Value(1)).current;
  const pulsing = phase === 'connecting' && !reduced;

  useEffect(() => {
    opacity.setValue(1);
    if (!pulsing) return undefined;
    const half = PULSE_MS / 2;
    const motion = Animated.loop(
      Animated.sequence([
        Animated.timing(opacity, { toValue: PULSE_LOW, duration: half, useNativeDriver: true }),
        Animated.timing(opacity, { toValue: 1, duration: half, useNativeDriver: true }),
      ]),
    );
    motion.start();
    return () => motion.stop();
  }, [opacity, pulsing]);

  const color = t[DOT_TONE[phase]];
  return (
    <Animated.View
      accessibilityRole="text"
      accessibilityLabel={phase}
      style={{ width: 7, height: 7, borderRadius: 3.5, flexShrink: 0, backgroundColor: color, opacity, boxShadow: [{ offsetX: 0, offsetY: 0, blurRadius: 0, spreadDistance: 3, color: mix(color, 20) }] }}
    />
  );
}

export function Swatch({ color, size, radius }: { color: string; size: number; radius: number }) {
  return <View style={{ width: size, height: size, borderRadius: radius, backgroundColor: color, flexShrink: 0 }} />;
}
