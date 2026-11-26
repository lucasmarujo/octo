import { storage } from './dom.js';

const DEVICE_KEY = 'octo.device';
const OPEN_KEY = 'octo.openAgent';
const SCOPE = '@';

export function scopedKey(key, device) {
  return device ? `${key}${SCOPE}${device}` : key;
}

export function scopedKeysOf(keys, device) {
  return device ? keys.filter((key) => key.endsWith(`${SCOPE}${device}`)) : [];
}

const current = storage.get(DEVICE_KEY);

export function activeDevice() {
  return current;
}

export const deviceStorage = {
  get: (key) => storage.get(scopedKey(key, current)),
  set: (key, value) => storage.set(scopedKey(key, current), value),
  json: (key, fallback) => storage.json(scopedKey(key, current), fallback),
};

export function switchDevice(id, agent = null) {
  storage.set(DEVICE_KEY, id);
  if (agent !== null) window.sessionStorage.setItem(OPEN_KEY, String(agent));
  window.location.reload();
}

export function agentToReopen() {
  const agent = window.sessionStorage.getItem(OPEN_KEY);
  window.sessionStorage.removeItem(OPEN_KEY);
  return agent === null ? null : Number(agent);
}

export function forgetScoped(device) {
  try {
    const keys = Array.from({ length: window.localStorage.length }, (_, index) => window.localStorage.key(index));
    for (const key of scopedKeysOf(keys, device)) window.localStorage.removeItem(key);
  } catch (err) {
    return;
  }
}
