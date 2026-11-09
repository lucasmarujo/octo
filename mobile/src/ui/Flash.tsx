import React, { useEffect, useRef } from 'react';
import type { StyleProp, ViewStyle } from 'react-native';
import { Animated } from 'react-native';

import type { FlashTone } from '../core/link';
import { EASE_OUT, SLOW_MS } from '../theme/motion';
import { useReduced } from '../theme/ThemeContext';

type Props = {
  tone: FlashTone | null;
  style?: StyleProp<ViewStyle>;
  children?: React.ReactNode;
};

const PEAK = 0.4;
const SWELL = 1.015;

export function Flash({ tone, style, children }: Props) {
  const reduced = useReduced();
  const progress = useRef(new Animated.Value(0)).current;

  useEffect(() => {
    progress.setValue(0);
    if (!tone || reduced) return undefined;
    const motion = Animated.timing(progress, { toValue: 1, duration: SLOW_MS, easing: EASE_OUT, useNativeDriver: true });
    motion.start();
    return () => motion.stop();
  }, [progress, tone, reduced]);

  const scale = progress.interpolate({ inputRange: [0, PEAK, 1], outputRange: [1, SWELL, 1] });
  return <Animated.View style={[style, { transform: [{ scale }] }]}>{children}</Animated.View>;
}
