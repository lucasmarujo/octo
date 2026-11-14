import * as Notifications from 'expo-notifications';
import { AppState, Platform } from 'react-native';

import app from '../../app.json';
import { devices, findDevice } from './devices';
import type { Links } from './links';
import { openDevice } from './nav';
import { latestMoves, movedTarget, push, pushTarget, shown } from './push';

const CHANNEL = 'agents';
const CHANNEL_NAME = 'Agents';
let asked = false;

function open(links: Links, data: unknown): void {
  const moved = movedTarget(data);
  if (moved && findDevice(moved.device)) {
    links.follow(moved);
    openDevice(moved.device);
    return;
  }
  const target = pushTarget(data);
  if (target && findDevice(target.device)) openDevice(target.device, target.agent);
}

async function followDelivered(links: Links): Promise<void> {
  try {
    const delivered = await Notifications.getPresentedNotificationsAsync();
    const found = latestMoves(delivered.map(note => ({ id: note.request.identifier, date: note.date, data: note.request.content.data })));
    for (const moved of found.moves) if (findDevice(moved.device)) links.follow(moved);
    await Promise.all(found.ids.map(id => Notifications.dismissNotificationAsync(id)));
  } catch {
    return;
  }
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
      const moved = movedTarget(notification.request.content.data);
      if (moved && findDevice(moved.device)) {
        links.follow(moved);
        const show = AppState.currentState !== 'active';
        return { shouldShowBanner: show, shouldShowList: show, shouldPlaySound: false, shouldSetBadge: false };
      }
      const target = pushTarget(notification.request.content.data);
      const phase = target ? links.of(target.device)?.store.get().phase || null : null;
      const show = shown(Boolean(target && findDevice(target.device)), AppState.currentState === 'active', phase);
      return { shouldShowBanner: show, shouldShowList: show, shouldPlaySound: show, shouldSetBadge: false };
    },
  });
  Notifications.addNotificationResponseReceivedListener(response => open(links, response.notification.request.content.data));
  const last = Notifications.getLastNotificationResponse();
  if (last) {
    open(links, last.notification.request.content.data);
    Notifications.clearLastNotificationResponse();
  }
  followDelivered(links);
  let paired = devices.get().length > 0;
  if (paired) syncPush(true);
  devices.subscribe(() => {
    const now = devices.get().length > 0;
    if (now && !paired) syncPush(true);
    paired = now;
  });
  AppState.addEventListener('change', state => {
    if (state !== 'active' || !devices.get().length) return;
    syncPush(false);
    followDelivered(links);
  });
}
