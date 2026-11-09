import { useEffect, useState } from 'react';
import { Keyboard, Platform } from 'react-native';

const SHOW = Platform.OS === 'ios' ? 'keyboardWillShow' : 'keyboardDidShow';
const HIDE = Platform.OS === 'ios' ? 'keyboardWillHide' : 'keyboardDidHide';

export function useKeyboard(): number {
  const [height, setHeight] = useState(0);
  useEffect(() => {
    const shown = Keyboard.addListener(SHOW, event => setHeight(event.endCoordinates.height));
    const hidden = Keyboard.addListener(HIDE, () => setHeight(0));
    return () => {
      shown.remove();
      hidden.remove();
    };
  }, []);
  return height;
}
