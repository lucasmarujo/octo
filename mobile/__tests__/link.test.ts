import type { Identity, Transport } from '../src/core/link';
import { DeviceLink, ownerOf, RETRY, RETRY_MAX } from '../src/core/link';
import { devices } from '../src/core/devices';
import { attentionOf, Links, STALE_AFTER_MS } from '../src/core/links';
import type { AgentView, Overview } from '../src/core/protocol';
import { OUTDATED, REPLACED, UNPAIRED } from '../src/core/protocol';
import { push, pushTarget, shown } from '../src/core/push';

jest.mock('../src/core/storage', () => ({ load: jest.fn(async () => null), save: jest.fn(async () => {}) }));

type Call = [method: 'open' | 'send' | 'close', id: string, ...rest: string[]];

const KEY = 'A'.repeat(43);
const IDENTITY: Identity = {
  agent: 'Octo/1.0.0 (Linux; Android 16; Pixel 9) Mobile',
  device: { type: 'device', model: 'Pixel 9', osVersion: '16', standalone: true, client: 'phone-1' },
};

function transport(): Transport & { calls: Call[] } {
  const calls: Call[] = [];
  return {
    calls,
    open: (id, url, key, agent) => calls.push(['open', id, url, key, agent]),
    send: (id, text) => calls.push(['send', id, text]),
    close: id => calls.push(['close', id]),
  };
}

function agent(part: Partial<AgentView>): AgentView {
  return {
    id: 1,
    session: 'main',
    name: 'holder-e2e',
    kind: 'claude',
    state: 'working',
    starting: false,
    activity: '',
    task: '',
    cwd: '/w',
    project: 'octo',
    color: '#e85eb6',
    startedAt: 1,
    stateSince: 1,
    turnStarted: 1,
    lastTurn: null,
    branch: null,
    exit: null,
    ...part,
  };
}

function overview(agents: AgentView[], now = 1000): Overview {
  return { host: 'lucas-desktop', now, session: 'main', sessions: [{ name: 'main', agents: agents.length }], agents, whereDir: '/w', dirs: {}, worktree: false, kinds: ['claude'] };
}

function opened(link: DeviceLink, wire: ReturnType<typeof transport>): string {
  link.connect();
  const open = wire.calls.filter(call => call[0] === 'open').pop();
  return open![1];
}

describe('a link to one computer', () => {
  beforeEach(() => jest.useFakeTimers());
  afterEach(() => jest.useRealTimers());

  it('opens the encrypted socket of the computer under a session of its own', () => {
    const wire = transport();
    const link = new DeviceLink('home', 'https://home-pc.trycloudflare.com', KEY, wire, IDENTITY);
    const session = opened(link, wire);
    expect(ownerOf(session)).toBe('home');
    expect(wire.calls[0]).toEqual(['open', session, 'wss://home-pc.trycloudflare.com/ws', KEY, IDENTITY.agent]);
    expect(link.store.get().phase).toBe('connecting');
  });

  it('tells the computer which phone it is once the channel is up, and nothing before', () => {
    const wire = transport();
    const link = new DeviceLink('home', 'https://h', KEY, wire, IDENTITY);
    const session = opened(link, wire);
    link.send({ type: 'close' });
    expect(wire.calls.filter(call => call[0] === 'send')).toEqual([]);
    link.onReady(session);
    expect(wire.calls.filter(call => call[0] === 'send')).toEqual([['send', session, JSON.stringify(IDENTITY.device)]]);
  });

  it('gives the computer the push token of the phone and its own name for that computer once notifications are allowed', () => {
    const wire = transport();
    const link = new DeviceLink('home', 'https://h', KEY, wire, IDENTITY);
    const session = opened(link, wire);
    const sent = () => wire.calls.filter(call => call[0] === 'send').map(call => JSON.parse(call[2]));
    link.announce('ExponentPushToken[abc]');
    expect(sent()).toEqual([]);
    link.onReady(session);
    expect(sent()).toEqual([{ ...IDENTITY.device, push: { token: 'ExponentPushToken[abc]', device: 'home' } }]);
    link.announce('ExponentPushToken[abc]');
    expect(sent()).toHaveLength(1);
    link.announce(null);
    expect(sent()[1]).toEqual({ ...IDENTITY.device, push: null });
  });

  it('goes live on the welcome and keeps the last workspace and usage', () => {
    const wire = transport();
    const link = new DeviceLink('home', 'https://h', KEY, wire, IDENTITY);
    const session = opened(link, wire);
    link.onReady(session);
    link.onMessage(session, JSON.stringify({ type: 'welcome', host: 'lucas-desktop', version: '0.2.31' }));
    link.onMessage(session, JSON.stringify({ type: 'state', ...overview([agent({})]) }));
    link.onMessage(session, JSON.stringify({ type: 'usage', entries: [{ kind: 'claude', windows: [], credits: null, problem: null }] }));
    link.onMessage(session, 'not json');
    const state = link.store.get();
    expect(state.phase).toBe('live');
    expect(state.overview!.host).toBe('lucas-desktop');
    expect(state.usage).toHaveLength(1);
  });

  it('stops for good when the computer forgot the phone, moved it or speaks another version', () => {
    for (const [code, phase] of [
      [UNPAIRED, 'unpaired'],
      [REPLACED, 'replaced'],
      [OUTDATED, 'outdated'],
    ] as const) {
      const wire = transport();
      const link = new DeviceLink('home', 'https://h', KEY, wire, IDENTITY);
      const session = opened(link, wire);
      link.onClosed(session, code);
      expect(link.store.get().phase).toBe(phase);
      jest.advanceTimersByTime(RETRY_MAX * 3);
      expect(wire.calls.filter(call => call[0] === 'open')).toHaveLength(1);
      link.reconnectNow();
      expect(wire.calls.filter(call => call[0] === 'open')).toHaveLength(1);
    }
  });

  it('retries a dropped connection a little later each time, up to a ceiling', () => {
    const wire = transport();
    const link = new DeviceLink('home', 'https://h', KEY, wire, IDENTITY);
    let session = opened(link, wire);
    const waits: number[] = [];
    for (let attempt = 0; attempt < RETRY.length + 2; attempt += 1) {
      link.onClosed(session, 1006);
      expect(link.store.get().phase).toBe('offline');
      const before = wire.calls.length;
      let waited = 0;
      while (wire.calls.length === before) {
        jest.advanceTimersByTime(100);
        waited += 100;
      }
      waits.push(waited);
      session = wire.calls[wire.calls.length - 1][1];
    }
    expect(waits).toEqual([...RETRY, RETRY_MAX, RETRY_MAX]);
  });

  it('ignores what an older session of the same computer says', () => {
    const wire = transport();
    const link = new DeviceLink('home', 'https://h', KEY, wire, IDENTITY);
    const old = opened(link, wire);
    link.onClosed(old, 1006);
    jest.advanceTimersByTime(RETRY[0]);
    const current = wire.calls[wire.calls.length - 1][1];
    expect(current).not.toBe(old);
    link.onMessage(old, JSON.stringify({ type: 'welcome', host: 'x', version: '1' }));
    link.onClosed(old, UNPAIRED);
    expect(link.store.get().phase).toBe('connecting');
  });

  it('opens a fresh socket when asked to restart, unless the computer turned the phone away', () => {
    const wire = transport();
    const link = new DeviceLink('home', 'https://h', KEY, wire, IDENTITY);
    const session = opened(link, wire);
    link.restart();
    expect(wire.calls).toContainEqual(['close', session]);
    expect(wire.calls.filter(call => call[0] === 'open')).toHaveLength(2);
    const fresh = wire.calls[wire.calls.length - 1][1];
    link.onClosed(fresh, UNPAIRED);
    link.restart();
    expect(wire.calls.filter(call => call[0] === 'open')).toHaveLength(2);
  });

  it('moves to the new address and key when the computer is paired again', () => {
    const wire = transport();
    const link = new DeviceLink('home', 'https://old', KEY, wire, IDENTITY);
    const session = opened(link, wire);
    link.onMessage(session, JSON.stringify({ type: 'state', ...overview([]) }));
    link.retarget('https://new', 'B'.repeat(43));
    expect(wire.calls).toContainEqual(['close', session]);
    expect(wire.calls[wire.calls.length - 1].slice(2, 4)).toEqual(['wss://new/ws', 'B'.repeat(43)]);
    expect(link.store.get().overview).toBeNull();
    expect(link.targets('https://new', 'B'.repeat(43))).toBe(true);
  });
});

describe('what deserves the attention of the user', () => {
  it('is an agent that starts waiting, or one that finished its turn', () => {
    const before = overview([agent({ id: 1, state: 'working' }), agent({ id: 2, state: 'working' }), agent({ id: 3, state: 'working', kind: 'terminal' }), agent({ id: 4, state: 'idle' })]);
    const after = overview([agent({ id: 1, state: 'paused' }), agent({ id: 2, state: 'done' }), agent({ id: 3, state: 'done', kind: 'terminal' }), agent({ id: 4, state: 'done' }), agent({ id: 5, state: 'paused' })]);
    const found = attentionOf('home', before, after).map(item => [item.device, item.agent.id, item.kind]);
    expect(found).toEqual([
      ['home', 1, 'needs'],
      ['home', 2, 'done'],
    ]);
    expect(attentionOf('home', null, after)).toEqual([]);
  });
});

describe('the notifications of the phone', () => {
  afterEach(() => push.set({ known: false, token: null }));

  it('reach every computer once the phone knows its token, and the ones paired later', () => {
    const wire = transport();
    const links = new Links(wire);
    devices.set([{ id: 'home', url: 'https://home', key: KEY, name: 'home', addedAt: 0, prefs: { who: null, worktree: false, bypass: false } }]);
    links.start(IDENTITY);
    const ready = () => {
      for (const call of wire.calls.filter(entry => entry[0] === 'open')) links.onReady(call[1]);
    };
    ready();
    const pushes = () => wire.calls.filter(call => call[0] === 'send').map(call => [ownerOf(call[1]), JSON.parse(call[2]).push]);
    expect(pushes()).toEqual([['home', undefined]]);
    push.set({ known: true, token: 'ExponentPushToken[abc]' });
    expect(pushes()).toEqual([
      ['home', undefined],
      ['home', { token: 'ExponentPushToken[abc]', device: 'home' }],
    ]);
    devices.set([...devices.get(), { id: 'work', url: 'https://work', key: KEY, name: 'work', addedAt: 0, prefs: { who: null, worktree: false, bypass: false } }]);
    ready();
    expect(pushes().filter(([device]) => device === 'work')).toEqual([['work', { token: 'ExponentPushToken[abc]', device: 'work' }]]);
    links.shut();
    devices.set([]);
  });

  it('stop for a computer the phone forgets, which is told so while it still listens', () => {
    const wire = transport();
    const links = new Links(wire);
    push.set({ known: true, token: 'ExponentPushToken[abc]' });
    devices.set([{ id: 'home', url: 'https://home', key: KEY, name: 'home', addedAt: 0, prefs: { who: null, worktree: false, bypass: false } }]);
    links.start(IDENTITY);
    const session = wire.calls.find(call => call[0] === 'open')![1];
    links.onReady(session);
    devices.set([]);
    const last = wire.calls.filter(call => call[0] === 'send').pop()!;
    expect(JSON.parse(last[2]).push).toBeNull();
    expect(wire.calls[wire.calls.length - 1]).toEqual(['close', session]);
    links.shut();
  });

  it('open the agent of the notification and stay out of the way while the app shows that computer', () => {
    expect(pushTarget({ device: 'home', agent: 4 })).toEqual({ device: 'home', agent: 4 });
    for (const junk of [null, undefined, 'home', { device: 'home' }, { device: 4, agent: 4 }, { device: 'home', agent: '4' }]) expect(pushTarget(junk)).toBeNull();
    expect(shown(true, true, 'live')).toBe(false);
    expect(shown(true, true, 'offline')).toBe(true);
    expect(shown(true, false, 'live')).toBe(true);
    expect(shown(true, true, null)).toBe(true);
    expect(shown(false, false, null)).toBe(false);
  });
});

describe('coming back to the app', () => {
  it('keeps the sockets after a short trip away and opens fresh ones after a long one', () => {
    const wire = transport();
    const links = new Links(wire);
    devices.set([
      { id: 'home', url: 'https://home', key: KEY, name: 'home', addedAt: 0, prefs: { who: null, worktree: false, bypass: false } },
      { id: 'work', url: 'https://work', key: KEY, name: 'work', addedAt: 0, prefs: { who: null, worktree: false, bypass: false } },
    ]);
    links.start(IDENTITY);
    const opens = () => wire.calls.filter(call => call[0] === 'open').length;
    expect(opens()).toBe(2);
    links.background(1000);
    links.foreground(1000 + STALE_AFTER_MS - 1);
    expect(opens()).toBe(2);
    links.background(5000);
    links.foreground(5000 + STALE_AFTER_MS);
    expect(opens()).toBe(4);
    expect(wire.calls.filter(call => call[0] === 'close')).toHaveLength(2);
    links.shut();
    devices.set([]);
  });
});
