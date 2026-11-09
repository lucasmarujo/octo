import React, { useCallback, useEffect, useState, useSyncExternalStore } from 'react';
import { StatusBar, View } from 'react-native';
import { SafeAreaProvider } from 'react-native-safe-area-context';

import type { Device } from './core/devices';
import { devices, findDevice } from './core/devices';
import { buzz } from './core/haptics';
import type { DeviceLink } from './core/link';
import type { Attention } from './core/links';
import type { Mood } from './core/model';
import model from './core/model';
import { nav, openDevice, openDevices, openPairing, watching } from './core/nav';
import { boot, links } from './core/runtime';
import { useStore } from './core/store';
import { DeviceScreen } from './screens/device/DeviceScreen';
import { Devices } from './screens/Devices';
import { Pair } from './screens/Pair';
import { openAppearance } from './sheets/Appearance';
import { openDeviceMenu } from './sheets/DeviceMenu';
import { ThemeProvider, useTheme } from './theme/ThemeContext';
import { SheetHost } from './ui/Sheet';
import { toast, ToastHost } from './ui/Toast';

function useMood(all: ReadonlyMap<string, DeviceLink>): Mood {
  const subscribe = useCallback(
    (listener: () => void) => {
      const stops = [...all.values()].map(link => link.store.subscribe(listener));
      return () => stops.forEach(stop => stop());
    },
    [all],
  );
  const snapshot = useCallback(() => model.moodOf([...all.values()].flatMap(link => link.store.get().overview?.agents || [])), [all]);
  return useSyncExternalStore(subscribe, snapshot);
}

function notice({ device, agent, kind }: Attention): void {
  const seen = watching.get();
  if (seen && seen.device === device && seen.agent === agent.id) return;
  const elsewhere = seen && seen.device === device ? '' : ` on ${findDevice(device)?.name || 'another computer'}`;
  const open = () => openDevice(device, agent.id);
  if (kind === 'needs') toast(`is waiting for you${elsewhere}`, 'warn', { title: agent.name, action: { label: 'Open', run: open } });
  else toast(`finished${elsewhere}`, 'success', { title: agent.name, action: { label: 'View', run: open } });
  buzz(kind);
}

function Shell() {
  const t = useTheme();
  const [ready, setReady] = useState(false);
  const list = useStore(devices);
  const all = useStore(links.store);
  const route = useStore(nav);
  const mood = useMood(all);

  useEffect(() => {
    let mounted = true;
    boot().then(
      () => {
        if (mounted) setReady(true);
      },
      () => {
        if (mounted) setReady(true);
      },
    );
    const stop = links.notice(notice);
    return () => {
      mounted = false;
      stop();
    };
  }, []);

  const paired = (device: Device) => openDevice(device.id);
  const menu = (device: Device) => openDeviceMenu({ device, onPair: id => openPairing(id), onForgotten: openDevices });

  let screen: React.ReactNode = null;
  if (ready) {
    const device = route.name === 'device' ? list.find(entry => entry.id === route.id) : undefined;
    const link = device ? all.get(device.id) : undefined;
    if (!list.length) screen = <Pair kind="new" onPaired={paired} />;
    else if (route.name === 'pair') screen = <Pair key={route.replace || 'new'} kind="new" replace={route.replace} onBack={openDevices} onPaired={paired} />;
    else if (route.name === 'device' && device && link) screen = <DeviceScreen key={device.id} device={device} link={link} agent={route.agent} at={route.at} onBack={openDevices} onMenu={() => menu(device)} onPaired={paired} />;
    else screen = <Devices devices={list} links={all} mood={mood} onOpen={device => openDevice(device.id)} onMenu={menu} onPair={() => openPairing()} onSettings={() => openAppearance()} />;
  }

  return (
    <View style={{ flex: 1, backgroundColor: t.bg }}>
      <StatusBar barStyle={t.light ? 'dark-content' : 'light-content'} />
      {screen}
      <SheetHost />
      <ToastHost />
    </View>
  );
}

export default function App() {
  return (
    <SafeAreaProvider>
      <ThemeProvider>
        <Shell />
      </ThemeProvider>
    </SafeAreaProvider>
  );
}
