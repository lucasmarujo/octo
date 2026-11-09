import * as Haptics from 'expo-haptics';

import { settings } from './settings';

export type Buzz = 'tap' | 'done' | 'needs' | 'arm';

const PLAY: Record<Buzz, () => Promise<void>> = {
  tap: () => Haptics.impactAsync(Haptics.ImpactFeedbackStyle.Light),
  done: () => Haptics.notificationAsync(Haptics.NotificationFeedbackType.Success),
  needs: () => Haptics.notificationAsync(Haptics.NotificationFeedbackType.Warning),
  arm: () => Haptics.impactAsync(Haptics.ImpactFeedbackStyle.Medium),
};

export function buzz(kind: Buzz): void {
  if (settings.get().haptics) PLAY[kind]().catch(() => {});
}
