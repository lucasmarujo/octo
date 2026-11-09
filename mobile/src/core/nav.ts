import { Store } from './store';

export type Route = { name: 'devices' } | { name: 'pair'; replace?: string } | { name: 'device'; id: string; agent?: number; at?: number };

export type Watching = {
  device: string;
  agent: number | null;
};

export const nav = new Store<Route>({ name: 'devices' });
export const watching = new Store<Watching | null>(null);

export function openDevices(): void {
  nav.set({ name: 'devices' });
}

export function openPairing(replace?: string): void {
  nav.set({ name: 'pair', replace });
}

export function openDevice(id: string, agent?: number): void {
  nav.set({ name: 'device', id, agent, at: Date.now() });
}
