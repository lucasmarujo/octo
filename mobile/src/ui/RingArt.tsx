import React from 'react';
import { StyleSheet, Text, View } from 'react-native';
import Svg, { Circle } from 'react-native-svg';

import model from '../core/model';
import type { UsageEntry } from '../core/protocol';
import { mix } from '../theme/color';
import { EASE_OUT, lasting, SLOW_MS } from '../theme/motion';
import { useReduced, useTheme } from '../theme/ThemeContext';
import type { Theme } from '../theme/tokens';
import { mono } from '../theme/type';
import { kindColor } from './atoms';
import { ProviderMark } from './ProviderMark';
import { useTween } from './tween';

type Props = {
  entry: UsageEntry;
  size: number;
  stroke: number;
  markSize: number;
  children?: React.ReactNode;
};

const VIEW = 56;
const CENTER = 28;
const OUTER = 25;
const INNER = 18.5;
const DASHED_STROKE = 2;
const DASHED = [3, 5];
const TURN = 2 * Math.PI;
const ICON_SCALE = 1.15;

export function usageColor(t: Theme, used: number): string {
  return { green: t.done, yellow: t.waiting, red: t.lost }[model.usageTone(used)];
}

function Fill({ used, radius, stroke }: { used: number; radius: number; stroke: number }) {
  const t = useTheme();
  const reduced = useReduced();
  const share = useTween(Math.min(100, Math.max(0, used)) / 100, lasting(SLOW_MS, reduced), EASE_OUT, 0);
  const length = TURN * radius;
  if (share <= 0) return null;
  return <Circle cx={CENTER} cy={CENTER} r={radius} fill="none" stroke={usageColor(t, used)} strokeWidth={stroke} strokeLinecap="round" strokeDasharray={[length * share, length]} />;
}

export function RingArt({ entry, size, stroke, markSize, children }: Props) {
  const t = useTheme();
  const windows = entry.windows.slice(0, 2);
  const funds = !windows.length && entry.credits !== null;
  const markStyle = [mono(markSize, 700), { color: kindColor(t, entry.kind) }];
  const mark = funds ? <Text style={markStyle}>{`$${Math.round(entry.credits!)}`}</Text> : <ProviderMark kind={entry.kind} size={Math.round(markSize * ICON_SCALE)} style={markStyle} />;
  return (
    <View style={{ width: size, height: size, flexShrink: 0 }}>
      <Svg width={size} height={size} viewBox={`0 0 ${VIEW} ${VIEW}`} style={{ transform: [{ rotate: '-90deg' }] }}>
        {windows.map((window, index) => {
          const radius = index ? INNER : OUTER;
          return (
            <React.Fragment key={window.label}>
              <Circle cx={CENTER} cy={CENTER} r={radius} fill="none" stroke={t.track} strokeWidth={stroke} />
              <Fill used={window.used} radius={radius} stroke={stroke} />
            </React.Fragment>
          );
        })}
        {funds ? <Circle cx={CENTER} cy={CENTER} r={OUTER} fill="none" stroke={mix(t.text, 25)} strokeWidth={stroke} /> : null}
        {!windows.length && !funds ? <Circle cx={CENTER} cy={CENTER} r={OUTER} fill="none" stroke={t.track} strokeWidth={DASHED_STROKE} strokeDasharray={DASHED} /> : null}
      </Svg>
      <View style={[StyleSheet.absoluteFill, { alignItems: 'center', justifyContent: 'center' }]}>{children || mark}</View>
    </View>
  );
}
