import * as Notifications from 'expo-notifications';
import { AppState, Platform } from 'react-native';

import app from '../../app.json';
import { devices, findDevice } from './devices';
import type { Links } from './links';
import { openDevice } from './nav';
import { push, pushTarget, shown } from './push';

const CHANNEL = 'agents';
const CHANNEL_NAME = 'Agents';
let asked = false;

function open(data: unknown): void {
  const target = pushTarget(data);
  if (target && findDevice(target.device)) openDevice(target.device, target.agent);
}

export async function syncPush(ask: boolean): Promise<void> {
  try {
    if (Platform.OS === 'android') await Notifications.setNotificationChannelAsync(CHANNEL, { name: CHANNEL_NAME, importance: Notifications.AndroidImportance.HIGH });
    let status = await Notifications.getPermissionsAsync();
    if (!status.granted && ask && status.canAskAgain && !asked) {
      asked = true;
      status = await Notifications.requestPermissionsAsync();
    }
    if (!status.granted) {
      push.set({ known: true, token: null });
      return;
    }
    if (push.get().token) return;
    const token = await Notifications.getExpoPushTokenAsync({ projectId: app.expo.extra.eas.projectId });
    push.set({ known: true, token: token.data });
  } catch {
    return;
  }
}

export function startNotifications(links: Links): void {
  Notifications.setNotificationHandler({
    handleNotification: async notification => {
      const target = pushTarget(notification.request.content.data);
      const phase = target ? links.of(target.device)?.store.get().phase || null : null;
      const show = shown(Boolean(target && findDevice(target.device)), AppState.currentState === 'active', phase);
      return { shouldShowBanner: show, shouldShowList: show, shouldPlaySound: show, shouldSetBadge: false };
    },
  });
  Notifications.addNotificationResponseReceivedListener(response => open(response.notification.request.content.data));
  const last = Notifications.getLastNotificationResponse();
  if (last) {
    open(last.notification.request.content.data);
    Notifications.clearLastNotificationResponse();
  }
  let paired = devices.get().length > 0;
  if (paired) syncPush(true);
  devices.subscribe(() => {
    const now = devices.get().length > 0;
    if (now && !paired) syncPush(true);
    paired = now;
  });
  AppState.addEventListener('change', state => {
    if (state === 'active' && devices.get().length) syncPush(false);
  });
}
