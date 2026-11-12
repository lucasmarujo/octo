import { useEffect, useRef } from 'react';
import type { StyleProp, ViewStyle } from 'react-native';
import { Animated, Easing, View } from 'react-native';
import Svg, { Defs, LinearGradient, Path, Stop } from 'react-native-svg';

import type { Mood } from '../core/model';
import { mix } from '../theme/color';
import { useReduced, useTheme } from '../theme/ThemeContext';
import { Kai } from './Kai';

type MarkProps = {
  size: number;
  turning?: boolean;
  style?: StyleProp<ViewStyle>;
};

type BrandProps = {
  mood: Mood;
  size: number;
  style?: StyleProp<ViewStyle>;
};

const OLD_THEME = 'old';
const VIEW_BOX = '60 60 392 392';
const RING = 'M72 256 A184 184 0 1 1 440 256 A184 184 0 1 1 72 256 Z M128 256 A128 128 0 1 1 384 256 A128 128 0 1 1 128 256 Z';
const CREST =
  'M72 256 A184 184 0 1 1 440 256 A184 184 0 1 1 72 256 Z M384.1 256 C382.9 261.5 380.4 267.1 377.4 272 C374.4 276.9 370.3 281.5 366.1 285.5 C362 289.5 357.1 292.9 352.6 296 C348.1 299.1 343.4 301.6 339.2 304 C335 306.5 331 308.5 327.5 310.9 C324 313.2 321 315.5 318.2 318.2 C315.5 321 313.2 324 310.9 327.5 C308.5 331 306.5 335 304 339.2 C301.6 343.4 299.1 348.1 296 352.6 C292.9 357.1 289.5 362 285.5 366.1 C281.5 370.3 276.9 374.4 272 377.4 C267.1 380.4 261.5 382.9 256 384.1 C250.5 385.4 244.6 385.3 239 384.9 C233.4 384.5 227.8 383 222.4 381.6 C216.9 380.1 211.5 378.3 206.3 376.1 C201 373.9 195.9 371.4 191 368.6 C186.1 365.8 181.3 362.6 176.9 359.1 C172.4 355.7 168.1 351.9 164.1 347.9 C160.1 343.9 156.3 339.6 152.9 335.1 C149.4 330.7 146.2 325.9 143.4 321 C140.6 316.1 138.1 311 135.9 305.7 C133.7 300.5 131.9 295.1 130.4 289.6 C129 284.2 127.5 278.6 127.1 273 C126.7 267.4 126.6 261.5 127.9 256 C129.1 250.5 131.6 244.9 134.6 240 C137.6 235.1 141.7 230.5 145.9 226.5 C150 222.5 154.9 219.1 159.4 216 C163.9 212.9 168.6 210.4 172.8 208 C177 205.5 181 203.5 184.5 201.1 C188 198.8 191 196.5 193.8 193.8 C196.5 191 198.8 188 201.1 184.5 C203.5 181 205.5 177 208 172.8 C210.4 168.6 212.9 163.9 216 159.4 C219.1 154.9 222.5 150 226.5 145.9 C230.5 141.7 235.1 137.6 240 134.6 C244.9 131.6 250.5 129.1 256 127.9 C261.5 126.6 267.4 126.7 273 127.1 C278.6 127.5 284.2 129 289.6 130.4 C295.1 131.9 300.5 133.7 305.7 135.9 C311 138.1 316.1 140.6 321 143.4 C325.9 146.2 330.7 149.4 335.1 152.9 C339.6 156.3 343.9 160.1 347.9 164.1 C351.9 168.1 355.7 172.4 359.1 176.9 C362.6 181.3 365.8 186.1 368.6 191 C371.4 195.9 373.9 201 376.1 206.3 C378.3 211.5 380.1 216.9 381.6 222.4 C383 227.8 384.5 233.4 384.9 239 C385.3 244.6 385.4 250.5 384.1 256 Z';
const CREST_SHARE = 42;
const GLOW_SHARE = 30;
const SHADE_SHARE = 66;
const ACCENT_AT = 0.42;
const TURN_MS = 2400;
const BLACK = '#000000';
const WHITE = '#ffffff';

export function markColors(accent: string): { crest: string; glow: string; shade: string } {
  return { crest: mix(accent, CREST_SHARE, BLACK), glow: mix(accent, GLOW_SHARE, WHITE), shade: mix(accent, SHADE_SHARE, BLACK) };
}

export function Mark({ size, turning = false, style }: MarkProps) {
  const t = useTheme();
  const reduced = useReduced();
  const turn = useRef(new Animated.Value(0)).current;
  const moving = turning && !reduced;

  useEffect(() => {
    turn.setValue(0);
    if (!moving) return undefined;
    const motion = Animated.loop(Animated.timing(turn, { toValue: 1, duration: TURN_MS, easing: Easing.linear, useNativeDriver: true }));
    motion.start();
    return () => motion.stop();
  }, [turn, moving]);

  const colors = markColors(t.accent);
  const rotate = turn.interpolate({ inputRange: [0, 1], outputRange: ['0deg', '360deg'] });
  return (
    <Animated.View pointerEvents="none" style={[{ width: size, height: size, transform: [{ rotate }] }, style]} accessibilityElementsHidden importantForAccessibility="no-hide-descendants">
      <Svg width={size} height={size} viewBox={VIEW_BOX}>
        <Defs>
          <LinearGradient id="ring" x1="110" y1="80" x2="410" y2="440" gradientUnits="userSpaceOnUse">
            <Stop offset="0" stopColor={colors.glow} />
            <Stop offset={ACCENT_AT} stopColor={t.accent} />
            <Stop offset="1" stopColor={colors.shade} />
          </LinearGradient>
        </Defs>
        <Path d={CREST} fill={colors.crest} fillRule="evenodd" />
        <Path d={RING} fill="url(#ring)" fillRule="evenodd" />
      </Svg>
    </Animated.View>
  );
}

export function Brand({ mood, size, style }: BrandProps) {
  const t = useTheme();
  if (t.name === OLD_THEME) return <Kai mood={mood} size={size} style={style} />;
  return (
    <View style={style}>
      <Mark size={size} turning={mood === 'working'} />
    </View>
  );
}
