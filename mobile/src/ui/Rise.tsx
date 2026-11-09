import React, { useEffect, useRef } from 'react';
import type { StyleProp, ViewStyle } from 'react-native';
import { Animated } from 'react-native';

import { BASE_MS, EASE_OUT, lasting } from '../theme/motion';
import { useReduced } from '../theme/ThemeContext';

type Props = {
  replay?: number | string;
  delay?: number;
  from?: number;
  shrink?: number;
  style?: StyleProp<ViewStyle>;
  children?: React.ReactNode;
};

const RISE_FROM = 10;
const RISE_SCALE = 0.98;

export function Rise({ replay = 0, delay = 0, from = RISE_FROM, shrink = RISE_SCALE, style, children }: Props) {
  const reduced = useReduced();
  const progress = useRef(new Animated.Value(0)).current;

  useEffect(() => {
    progress.setValue(0);
    const motion = Animated.timing(progress, { toValue: 1, duration: lasting(BASE_MS, reduced), delay: reduced ? 0 : delay, easing: EASE_OUT, useNativeDriver: true });
    motion.start();
    return () => motion.stop();
  }, [progress, replay, delay, reduced]);

  const translateY = progress.interpolate({ inputRange: [0, 1], outputRange: [from, 0] });
  const scale = progress.interpolate({ inputRange: [0, 1], outputRange: [shrink, 1] });
  return <Animated.View style={[style, { opacity: progress, transform: [{ translateY }, { scale }] }]}>{children}</Animated.View>;
}
