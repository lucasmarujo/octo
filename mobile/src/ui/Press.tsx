import React, { useRef, useState } from 'react';
import type { GestureResponderEvent, StyleProp, ViewStyle } from 'react-native';
import { Animated, Pressable } from 'react-native';

import { EASE_OUT, EASE_SPRING, FAST_MS, PRESS_MS } from '../theme/motion';

type Props = {
  onPress?: (event: GestureResponderEvent) => void;
  onLongPress?: () => void;
  style?: StyleProp<ViewStyle>;
  held?: StyleProp<ViewStyle>;
  label?: string;
  role?: 'button' | 'tab' | 'switch';
  selected?: boolean;
  disabled?: boolean;
  children?: React.ReactNode;
};

const PRESSED_SCALE = 0.97;
const PRESSED_FILTER = [{ brightness: 0.88 }];
const Touchable = Animated.createAnimatedComponent(Pressable);

export function Press({ onPress, onLongPress, style, held, label, role = 'button', selected, disabled, children }: Props) {
  const scale = useRef(new Animated.Value(1)).current;
  const [down, setDown] = useState(false);

  const pressIn = () => {
    setDown(true);
    Animated.timing(scale, { toValue: PRESSED_SCALE, duration: PRESS_MS, easing: EASE_OUT, useNativeDriver: true }).start();
  };
  const pressOut = () => {
    setDown(false);
    Animated.timing(scale, { toValue: 1, duration: FAST_MS, easing: EASE_SPRING, useNativeDriver: true }).start();
  };

  return (
    <Touchable
      onPress={onPress}
      onLongPress={onLongPress}
      onPressIn={pressIn}
      onPressOut={pressOut}
      disabled={disabled}
      accessibilityRole={role}
      accessibilityLabel={label}
      accessibilityState={{ selected, disabled }}
      style={[style, down && { filter: PRESSED_FILTER }, down && held, { transform: [{ scale }] }]}>
      {children}
    </Touchable>
  );
}
