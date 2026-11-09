import type { Pairing } from './pairing';
import { hostOf } from './pairing';
import type { AgentKind } from './protocol';
import { load, save } from './storage';
import { Store } from './store';

export type DevicePrefs = {
  who: AgentKind | null;
  worktree: boolean;
  bypass: boolean;
};

export type Device = {
  id: string;
  url: string;
  key: string;
  name: string;
  addedAt: number;
  prefs: DevicePrefs;
};

export type Paired = {
  list: Device[];
  device: Device;
  added: boolean;
};

const NAME = 'devices';
const ID_BYTES = 16;
const PREFS: DevicePrefs = { who: null, worktree: false, bypass: false };

export const devices = new Store<Device[]>([]);

export function newId(): string {
  let id = '';
  for (let i = 0; i < ID_BYTES; i += 1) {
    id += Math.floor(Math.random() * 256)
      .toString(16)
      .padStart(2, '0');
  }
  return id;
}

function isDevice(entry: unknown): entry is Device {
  const device = entry as Device | null;
  return Boolean(device) && typeof device!.id === 'string' && typeof device!.url === 'string' && typeof device!.key === 'string';
}

export function readDevices(saved: unknown): Device[] {
  if (!Array.isArray(saved)) return [];
  return saved.filter(isDevice).map(entry => ({
    id: entry.id,
    url: entry.url,
    key: entry.key,
    name: typeof entry.name === 'string' && entry.name ? entry.name : hostOf(entry.url),
    addedAt: Number(entry.addedAt) || 0,
    prefs: { ...PREFS, ...(entry.prefs || {}) },
  }));
}

export function paired(list: Device[], pairing: Pairing, id: string, now: number): Paired {
  const known = list.find(device => device.key === pairing.key) || list.find(device => device.url === pairing.url);
  if (known) {
    const device = { ...known, url: pairing.url, key: pairing.key };
    return { list: list.map(entry => (entry.id === known.id ? device : entry)), device, added: false };
  }
  const device: Device = { id, url: pairing.url, key: pairing.key, name: hostOf(pairing.url), addedAt: now, prefs: PREFS };
  return { list: [...list, device], device, added: true };
}

export function repaired(list: Device[], id: string, pairing: Pairing): Paired | null {
  const target = list.find(device => device.id === id);
  if (!target) return null;
  const device = { ...target, url: pairing.url, key: pairing.key };
  const rest = list.filter(entry => entry.id === id || (entry.key !== pairing.key && entry.url !== pairing.url));
  return { list: rest.map(entry => (entry.id === id ? device : entry)), device, added: false };
}

function keep(list: Device[]): void {
  devices.set(list);
  save(NAME, list).catch(() => {});
}

export async function loadDevices(): Promise<void> {
  devices.set(readDevices(await load<unknown>(NAME)));
}

export function pairDevice(pairing: Pairing): Paired {
  const result = paired(devices.get(), pairing, newId(), Date.now());
  keep(result.list);
  return result;
}

export function repairDevice(id: string, pairing: Pairing): Paired {
  const result = repaired(devices.get(), id, pairing) || paired(devices.get(), pairing, newId(), Date.now());
  keep(result.list);
  return result;
}

export function forgetDevice(id: string): void {
  keep(devices.get().filter(device => device.id !== id));
}

export function changeDevice(id: string, part: Partial<Pick<Device, 'name' | 'prefs'>>): void {
  const list = devices.get();
  const known = list.find(device => device.id === id);
  if (!known) return;
  const next = { ...known, ...part };
  if (next.name === known.name && next.prefs === known.prefs) return;
  keep(list.map(device => (device.id === id ? next : device)));
}

export function findDevice(id: string | null): Device | null {
  return devices.get().find(device => device.id === id) || null;
}
