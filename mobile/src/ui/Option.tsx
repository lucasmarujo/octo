import React, { useEffect, useRef, useState } from 'react';
import { Animated, Easing, StyleSheet, Text, View } from 'react-native';
import Svg, { Circle } from 'react-native-svg';

import { buzz } from '../core/haptics';
import { mix } from '../theme/color';
import { EASE_OUT } from '../theme/motion';
import { FILL, useStyles } from '../theme/styles';
import { useTheme } from '../theme/ThemeContext';
import type { Theme } from '../theme/tokens';
import { mono, sans } from '../theme/type';
import { Press } from './Press';
import { useTween } from './tween';

export type IconTone = 'plain' | 'link' | 'accent' | 'red' | 'green' | 'bare';

type OptionProps = {
  icon?: (color: string) => React.ReactNode;
  iconTone?: IconTone;
  iconFill?: string;
  text: string;
  detail?: string | null;
  tone?: 'danger' | 'current';
  monoLabel?: boolean;
  trail?: React.ReactNode;
  chevron?: boolean;
  first?: boolean;
  onPress: () => void;
};

type ArmedProps = Omit<OptionProps, 'onPress' | 'trail' | 'chevron'> & {
  armed: string;
  action: () => void;
};

const CONFIRM_MS = 3000;
const FILL_MS = 180;
const SEPARATOR_LEFT = 58;
const COUNTDOWN = 28;
const COUNTDOWN_RADIUS = 11;
const COUNTDOWN_STROKE = 3;
const COUNTDOWN_LENGTH = 2 * Math.PI * COUNTDOWN_RADIUS;

const styles = (t: Theme) =>
  StyleSheet.create({
    group: { borderRadius: 18, backgroundColor: t.surface, borderWidth: 1, borderColor: t.border, overflow: 'hidden' },
    option: { flexDirection: 'row', alignItems: 'center', gap: 12, minHeight: 56, paddingVertical: 10, paddingHorizontal: 14, overflow: 'hidden' },
    held: { backgroundColor: t.surfaceRaised },
    separator: { position: 'absolute', top: 0, left: SEPARATOR_LEFT, right: 0, height: 1, backgroundColor: t.border },
    icon: { width: 32, height: 32, borderRadius: 10, alignItems: 'center', justifyContent: 'center', flexShrink: 0 },
    label: { flex: 1, minWidth: 0 },
    text: { ...sans(16, 600), color: t.text },
    monoText: { ...mono(14), color: t.text },
    detail: { ...mono(11), color: t.textFaint },
    chevron: { ...sans(18), color: t.textFaint, flexShrink: 0 },
    fill: { ...FILL, backgroundColor: t.lost, transformOrigin: 'left' },
    trail: { ...mono(12), color: t.textFaint, flexShrink: 0 },
    note: { ...sans(13), color: t.textFaint, paddingHorizontal: 4 },
  });

function iconLook(t: Theme, tone: IconTone): { bg: string; ink: string } {
  return {
    plain: { bg: t.surfaceRaised, ink: t.text },
    link: { bg: mix(t.link, 14), ink: t.link },
    accent: { bg: mix(t.accent, 14), ink: t.accent },
    red: { bg: mix(t.lost, 14), ink: t.lost },
    green: { bg: mix(t.done, 14), ink: t.done },
    bare: { bg: 'transparent', ink: t.link },
  }[tone];
}

export function Group({ children }: { children: React.ReactNode }) {
  const s = useStyles(styles);
  const rows = React.Children.toArray(children).filter(Boolean);
  if (!rows.length) return null;
  return <View style={s.group}>{rows.map((row, index) => (React.isValidElement<{ first?: boolean }>(row) ? React.cloneElement(row, { first: index === 0 }) : row))}</View>;
}

export function Note({ text }: { text: string }) {
  const s = useStyles(styles);
  return <Text style={s.note}>{text}</Text>;
}

export function TrailText({ children }: { children: React.ReactNode }) {
  const s = useStyles(styles);
  return <Text style={s.trail}>{children}</Text>;
}

export function Option({ icon, iconTone = 'plain', iconFill, text, detail, tone, monoLabel, trail, chevron, first = true, onPress }: OptionProps) {
  const t = useTheme();
  const s = useStyles(styles);
  const look = iconLook(t, iconTone);
  const ink = tone === 'danger' ? t.lost : tone === 'current' ? t.accent : t.text;
  return (
    <Press style={s.option} held={s.held} onPress={onPress}>
      {first ? null : <View style={s.separator} />}
      {icon ? <View style={[s.icon, { backgroundColor: iconFill || look.bg }]}>{icon(look.ink)}</View> : null}
      <View style={s.label}>
        <Text style={[monoLabel ? s.monoText : s.text, { color: ink }]}>{text}</Text>
        {detail ? (
          <Text style={s.detail} numberOfLines={1}>
            {detail}
          </Text>
        ) : null}
      </View>
      {trail}
      {chevron ? <Text style={s.chevron}>›</Text> : null}
    </Press>
  );
}

export function ArmedOption({ icon, iconTone = 'red', text, detail, tone = 'danger', armed, action, first = true }: ArmedProps) {
  const t = useTheme();
  const s = useStyles(styles);
  const [on, setOn] = useState(false);
  const fill = useRef(new Animated.Value(0)).current;
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const left = useTween(on ? 0 : 1, on ? CONFIRM_MS : 0, Easing.linear, 1);

  useEffect(
    () => () => {
      if (timer.current) clearTimeout(timer.current);
    },
    [],
  );

  const disarm = () => {
    setOn(false);
    Animated.timing(fill, { toValue: 0, duration: FILL_MS, easing: EASE_OUT, useNativeDriver: true }).start();
  };
  const press = () => {
    if (on) {
      if (timer.current) clearTimeout(timer.current);
      action();
      return;
    }
    setOn(true);
    buzz('arm');
    Animated.timing(fill, { toValue: 1, duration: FILL_MS, easing: EASE_OUT, useNativeDriver: true }).start();
    timer.current = setTimeout(disarm, CONFIRM_MS);
  };

  const look = iconLook(t, iconTone);
  const ink = on ? t.lostInk : tone === 'danger' ? t.lost : t.text;
  return (
    <Press style={s.option} held={on ? undefined : s.held} onPress={press}>
      {first ? null : <View style={s.separator} />}
      <Animated.View style={[s.fill, { transform: [{ scaleX: fill }] }]} />
      {icon ? <View style={[s.icon, { backgroundColor: on ? mix(t.lostInk, 22) : look.bg }]}>{icon(on ? t.lostInk : look.ink)}</View> : null}
      <View style={s.label}>
        <Text style={[s.text, { color: ink }]}>{on ? armed : text}</Text>
        {detail ? (
          <Text style={s.detail} numberOfLines={1}>
            {detail}
          </Text>
        ) : null}
      </View>
      {on ? (
        <Svg width={COUNTDOWN} height={COUNTDOWN} viewBox={`0 0 ${COUNTDOWN} ${COUNTDOWN}`} style={{ transform: [{ rotate: '-90deg' }] }}>
          <Circle cx={COUNTDOWN / 2} cy={COUNTDOWN / 2} r={COUNTDOWN_RADIUS} fill="none" stroke={mix(t.lostInk, 25)} strokeWidth={COUNTDOWN_STROKE} />
          {left > 0 ? <Circle cx={COUNTDOWN / 2} cy={COUNTDOWN / 2} r={COUNTDOWN_RADIUS} fill="none" stroke={t.lostInk} strokeWidth={COUNTDOWN_STROKE} strokeLinecap="round" strokeDasharray={[COUNTDOWN_LENGTH * left, COUNTDOWN_LENGTH]} /> : null}
        </Svg>
      ) : null}
    </Press>
  );
}
