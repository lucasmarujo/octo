import { useEffect, useState } from 'react';
import { Keyboard, Platform } from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';

const SHOW = Platform.OS === 'ios' ? 'keyboardWillShow' : 'keyboardDidShow';
const HIDE = Platform.OS === 'ios' ? 'keyboardWillHide' : 'keyboardDidHide';

export function covered(height: number, navigation: number, os: string): number {
  if (height <= 0) return 0;
  return os === 'android' ? height + navigation : height;
}

export function useKeyboard(): number {
  const insets = useSafeAreaInsets();
  const [height, setHeight] = useState(0);
  useEffect(() => {
    const shown = Keyboard.addListener(SHOW, event => setHeight(event.endCoordinates.height));
    const hidden = Keyboard.addListener(HIDE, () => setHeight(0));
    return () => {
      shown.remove();
      hidden.remove();
    };
  }, []);
  return covered(height, insets.bottom, Platform.OS);
}
