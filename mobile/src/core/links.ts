import type { Device } from './devices';
import { changeDevice, devices } from './devices';
import type { Heard, Identity, Transport } from './link';
import { DeviceLink, ownerOf } from './link';
import type { AgentView, BridgeMsg, Overview } from './protocol';
import { Store } from './store';

export type Attention = {
  device: string;
  agent: AgentView;
  kind: 'needs' | 'done';
};

export type Noticed = (attention: Attention) => void;

export const SUSPEND_AFTER_MS = 30000;

export function attentionOf(device: string, previous: Overview | null, next: Overview): Attention[] {
  if (!previous) return [];
  const before = new Map(previous.agents.map(agent => [agent.id, agent.state]));
  const found: Attention[] = [];
  for (const agent of next.agents) {
    const was = before.get(agent.id);
    if (!was || was === agent.state) continue;
    if (agent.state === 'paused') found.push({ device, agent, kind: 'needs' });
    else if (agent.state === 'done' && was === 'working' && agent.kind !== 'terminal') found.push({ device, agent, kind: 'done' });
  }
  return found;
}

export class Links {
  readonly store = new Store<ReadonlyMap<string, DeviceLink>>(new Map());
  private readonly noticed = new Set<Noticed>();
  private timer: ReturnType<typeof setTimeout> | null = null;
  private stop: (() => void) | null = null;
  private identity: Identity | null = null;

  constructor(private readonly transport: Transport) {}

  start(identity: Identity): void {
    if (this.stop) return;
    this.identity = identity;
    this.stop = devices.subscribe(() => this.sync(devices.get()));
    this.sync(devices.get());
  }

  shut(): void {
    if (this.stop) this.stop();
    this.stop = null;
    this.clearTimer();
    for (const link of this.store.get().values()) link.dispose();
    this.store.set(new Map());
  }

  of(device: string | null): DeviceLink | null {
    return (device && this.store.get().get(device)) || null;
  }

  notice(noticed: Noticed): () => void {
    this.noticed.add(noticed);
    return () => {
      this.noticed.delete(noticed);
    };
  }

  onReady(session: string): void {
    this.of(ownerOf(session))?.onReady(session);
  }

  onMessage(session: string, text: string): void {
    this.of(ownerOf(session))?.onMessage(session, text);
  }

  onClosed(session: string, code: number): void {
    this.of(ownerOf(session))?.onClosed(session, code);
  }

  foreground(): void {
    this.clearTimer();
    for (const link of this.store.get().values()) link.reconnectNow();
  }

  background(): void {
    this.clearTimer();
    this.timer = setTimeout(() => {
      this.timer = null;
      for (const link of this.store.get().values()) link.suspend();
    }, SUSPEND_AFTER_MS);
  }

  private sync(list: Device[]): void {
    const identity = this.identity;
    if (!identity) return;
    const current = this.store.get();
    const next = new Map<string, DeviceLink>();
    let changed = current.size !== list.length;
    for (const device of list) {
      const known = current.get(device.id);
      if (known) {
        if (!known.targets(device.url, device.key)) known.retarget(device.url, device.key);
        next.set(device.id, known);
        continue;
      }
      const link = new DeviceLink(device.id, device.url, device.key, this.transport, identity);
      link.listen(this.hear(device.id));
      link.connect();
      next.set(device.id, link);
      changed = true;
    }
    for (const [id, link] of current) {
      if (!next.has(id)) link.dispose();
    }
    if (changed) this.store.set(next);
  }

  private hear(device: string): Heard {
    return (message: BridgeMsg, previous: Overview | null) => {
      if (message.type === 'welcome') changeDevice(device, { name: message.host });
      if (message.type !== 'state') return;
      for (const attention of attentionOf(device, previous, message)) {
        for (const noticed of [...this.noticed]) noticed(attention);
      }
    };
  }

  private clearTimer(): void {
    if (this.timer) clearTimeout(this.timer);
    this.timer = null;
  }
}
