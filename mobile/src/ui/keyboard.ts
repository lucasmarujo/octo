import { useEffect, useState } from 'react';
import { Keyboard, Platform } from 'react-native';
import type { KeyboardEvent } from 'react-native';
import { useSafeAreaFrame, useSafeAreaInsets } from 'react-native-safe-area-context';

type Shown = {
  height: number;
  top: number;
};

const SHOW = Platform.OS === 'ios' ? 'keyboardWillShow' : 'keyboardDidShow';
const HIDE = Platform.OS === 'ios' ? 'keyboardWillHide' : 'keyboardDidHide';
const MOVE = 'keyboardWillChangeFrame';

export function covered(height: number, navigation: number, os: string): number {
  if (height <= 0) return 0;
  return os === 'android' ? height + navigation : height;
}

export function overlap(bottom: number, top: number): number {
  return top > 0 ? Math.max(0, Math.round(bottom - top)) : 0;
}

export function hiddenBy(shown: Shown | null, bottom: number, navigation: number, os: string): number {
  if (!shown) return 0;
  return Math.max(covered(shown.height, navigation, os), overlap(bottom, shown.top));
}

export function useKeyboard(): number {
  const insets = useSafeAreaInsets();
  const frame = useSafeAreaFrame();
  const [shown, setShown] = useState<Shown | null>(null);
  useEffect(() => {
    const show = (event: KeyboardEvent) => setShown(event.endCoordinates.height > 0 ? { height: event.endCoordinates.height, top: event.endCoordinates.screenY } : null);
    const listeners = [Keyboard.addListener(SHOW, show), Keyboard.addListener(HIDE, () => setShown(null))];
    if (Platform.OS === 'ios') listeners.push(Keyboard.addListener(MOVE, show));
    return () => listeners.forEach(listener => listener.remove());
  }, []);
  return hiddenBy(shown, frame.y + frame.height, insets.bottom, Platform.OS);
}
