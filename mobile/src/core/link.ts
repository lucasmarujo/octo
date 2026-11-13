import { socketUrl } from './pairing';
import type { BridgeMsg, DeviceMsg, Overview, PhoneMsg, UsageEntry } from './protocol';
import { OUTDATED, REPLACED, UNPAIRED } from './protocol';
import { Store } from './store';

export type LinkPhase = 'connecting' | 'live' | 'offline' | 'unpaired' | 'replaced' | 'outdated';
export type FlashTone = 'needs' | 'done';

export type LinkState = {
  phase: LinkPhase;
  overview: Overview | null;
  usage: UsageEntry[] | null;
  skew: number;
};

export type Transport = {
  open(id: string, url: string, key: string, agent: string): void;
  send(id: string, text: string): void;
  queued(id: string): number;
  close(id: string): void;
};

export type Identity = {
  agent: string;
  device: DeviceMsg;
};

export type Heard = (message: BridgeMsg, previous: Overview | null) => void;

type Flash = {
  tone: FlashTone;
  at: number;
};

export const RETRY = [400, 1000, 2000, 4000, 8000];
export const RETRY_MAX = 10000;
export const FLASH_MS = 700;
const SEPARATOR = ':';
const STOPPED: LinkPhase[] = ['unpaired', 'replaced', 'outdated'];
const CLOSED: Record<number, LinkPhase> = { [UNPAIRED]: 'unpaired', [OUTDATED]: 'outdated', [REPLACED]: 'replaced' };

export function ownerOf(session: string): string {
  return session.slice(0, session.lastIndexOf(SEPARATOR));
}

export class DeviceLink {
  readonly store = new Store<LinkState>({ phase: 'connecting', overview: null, usage: null, skew: 0 });
  version: string | null = null;
  private session: string | null = null;
  private ready = false;
  private attempt = 0;
  private opened = 0;
  private timer: ReturnType<typeof setTimeout> | null = null;
  private pushToken: string | null | undefined = undefined;
  private readonly heard = new Set<Heard>();
  private readonly flashes = new Map<number, Flash>();

  constructor(
    readonly id: string,
    private url: string,
    private key: string,
    private readonly transport: Transport,
    private readonly identity: Identity,
  ) {}

  now(): number {
    return Date.now() - this.store.get().skew;
  }

  targets(url: string, key: string): boolean {
    return this.url === url && this.key === key;
  }

  retarget(url: string, key: string): void {
    this.drop();
    this.url = url;
    this.key = key;
    this.attempt = 0;
    this.store.set({ phase: 'connecting', overview: null, usage: null, skew: 0 });
    this.connect();
  }

  connect(): void {
    this.clearTimer();
    if (this.session) return;
    this.store.patch({ phase: 'connecting' });
    this.opened += 1;
    this.session = `${this.id}${SEPARATOR}${this.opened}`;
    this.ready = false;
    this.transport.open(this.session, socketUrl(this.url), this.key, this.identity.agent);
  }

  send(message: PhoneMsg): void {
    if (this.session && this.ready) this.transport.send(this.session, JSON.stringify(message));
  }

  backlog(): number {
    return this.session && this.ready ? this.transport.queued(this.session) : 0;
  }

  listen(heard: Heard): () => void {
    this.heard.add(heard);
    return () => {
      this.heard.delete(heard);
    };
  }

  retryNow(): void {
    this.attempt = 0;
    this.connect();
  }

  reconnectNow(): void {
    if (this.session || STOPPED.includes(this.store.get().phase)) return;
    this.retryNow();
  }

  restart(): void {
    if (STOPPED.includes(this.store.get().phase)) return;
    this.drop();
    this.retryNow();
  }

  dispose(): void {
    this.drop();
    this.heard.clear();
  }

  flashOf(agent: number): FlashTone | null {
    const flash = this.flashes.get(agent);
    return flash && Date.now() - flash.at < FLASH_MS ? flash.tone : null;
  }

  announce(token: string | null): void {
    if (this.pushToken === token) return;
    this.pushToken = token;
    if (this.ready) this.introduce();
  }

  onReady(session: string): void {
    if (session !== this.session) return;
    this.ready = true;
    this.introduce();
  }

  onMessage(session: string, text: string): void {
    if (session !== this.session) return;
    let message: BridgeMsg;
    try {
      message = JSON.parse(text) as BridgeMsg;
    } catch {
      return;
    }
    this.handle(message);
  }

  onClosed(session: string, code: number): void {
    if (session !== this.session) return;
    this.session = null;
    this.ready = false;
    const stopped = CLOSED[code];
    if (stopped) {
      this.store.patch({ phase: stopped });
      return;
    }
    this.store.patch({ phase: 'offline' });
    const delay = RETRY[this.attempt] || RETRY_MAX;
    this.attempt += 1;
    this.clearTimer();
    this.timer = setTimeout(() => this.connect(), delay);
  }

  private introduce(): void {
    const token = this.pushToken;
    if (token === undefined) this.send(this.identity.device);
    else this.send({ ...this.identity.device, push: token ? { token, device: this.id } : null });
  }

  private handle(message: BridgeMsg): void {
    const previous = this.store.get().overview;
    if (message.type === 'welcome') {
      this.version = message.version;
      this.attempt = 0;
      this.store.patch({ phase: 'live' });
    } else if (message.type === 'state') {
      this.mark(previous, message);
      this.store.patch({ skew: Date.now() - message.now, overview: message });
    } else if (message.type === 'usage') {
      this.store.patch({ usage: message.entries });
    }
    for (const heard of [...this.heard]) heard(message, previous);
  }

  private mark(previous: Overview | null, next: Overview): void {
    if (!previous) return;
    const before = new Map(previous.agents.map(agent => [agent.id, agent.state]));
    for (const agent of next.agents) {
      const was = before.get(agent.id);
      if (!was || was === agent.state) continue;
      if (agent.state === 'paused') this.flashes.set(agent.id, { tone: 'needs', at: Date.now() });
      if (agent.state === 'done') this.flashes.set(agent.id, { tone: 'done', at: Date.now() });
    }
  }

  private drop(): void {
    this.clearTimer();
    if (this.session) this.transport.close(this.session);
    this.session = null;
    this.ready = false;
  }

  private clearTimer(): void {
    if (this.timer) clearTimeout(this.timer);
    this.timer = null;
  }
}
