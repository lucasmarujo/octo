import { useEffect, useRef } from 'react';
import type { ImageSourcePropType, StyleProp, ViewStyle } from 'react-native';
import { Animated, View } from 'react-native';

import type { Mood } from '../core/model';
import { steps } from '../theme/motion';
import { useReduced } from '../theme/ThemeContext';

type Strip = {
  source: ImageSourcePropType;
  frames: number;
  ms: number;
  loop: boolean;
};

const STRIPS: Record<Mood, Strip> = {
  idle: { source: require('../../assets/kai/idle.png'), frames: 4, ms: 3600, loop: true },
  working: { source: require('../../assets/kai/working.png'), frames: 4, ms: 560, loop: true },
  needs: { source: require('../../assets/kai/needs.png'), frames: 4, ms: 900, loop: true },
  done: { source: require('../../assets/kai/done.png'), frames: 5, ms: 1200, loop: false },
  lost: { source: require('../../assets/kai/lost.png'), frames: 4, ms: 1600, loop: true },
};

type Props = {
  mood: Mood;
  size: number;
  style?: StyleProp<ViewStyle>;
};

export function Kai({ mood, size, style }: Props) {
  const reduced = useReduced();
  const progress = useRef(new Animated.Value(0)).current;
  const strip = STRIPS[mood];

  useEffect(() => {
    progress.setValue(0);
    if (reduced) return undefined;
    const pass = Animated.timing(progress, { toValue: 1, duration: strip.ms, easing: steps(strip.frames), useNativeDriver: true });
    const motion = strip.loop ? Animated.loop(pass) : pass;
    motion.start();
    return () => motion.stop();
  }, [progress, reduced, strip]);

  const translateX = progress.interpolate({ inputRange: [0, 1], outputRange: [0, -size * (strip.frames - 1)] });
  return (
    <View pointerEvents="none" style={[{ width: size, height: size, overflow: 'hidden' }, style]} accessibilityElementsHidden importantForAccessibility="no-hide-descendants">
      <Animated.Image source={strip.source} fadeDuration={0} resizeMode="stretch" style={{ width: size * strip.frames, height: size, transform: [{ translateX }] }} />
    </View>
  );
}
