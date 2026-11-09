import type { Device } from '../src/core/devices';
import { paired, readDevices, repaired } from '../src/core/devices';

jest.mock('../src/core/storage', () => ({ load: jest.fn(async () => null), save: jest.fn(async () => {}) }));

const HOME = { url: 'https://home-pc.trycloudflare.com', key: 'A'.repeat(43) };
const WORK = { url: 'https://work-pc.tail1234.ts.net', key: 'B'.repeat(43) };

describe('pairing a computer', () => {
  it('adds a new computer named after its address until it says its name', () => {
    const result = paired([], HOME, 'id-home', 10);
    expect(result.added).toBe(true);
    expect(result.list).toHaveLength(1);
    expect(result.device).toEqual({ id: 'id-home', url: HOME.url, key: HOME.key, name: 'home-pc.trycloudflare.com', addedAt: 10, prefs: { who: null, worktree: false, bypass: false } });
  });

  it('keeps every computer apart so one phone holds several', () => {
    const first = paired([], HOME, 'id-home', 10);
    const second = paired(first.list, WORK, 'id-work', 20);
    expect(second.added).toBe(true);
    expect(second.list.map(device => device.id)).toEqual(['id-home', 'id-work']);
  });

  it('scanning the same code again keeps the computer, its name and its choices', () => {
    const first = paired([], HOME, 'id-home', 10);
    const named: Device[] = [{ ...first.device, name: 'lucas-desktop', prefs: { who: 'codex', worktree: true, bypass: false } }];
    const again = paired(named, HOME, 'id-other', 30);
    expect(again.added).toBe(false);
    expect(again.list).toEqual(named);
  });

  it('a new code from the same computer replaces its key in place', () => {
    const first = paired([], HOME, 'id-home', 10);
    const fresh = { url: HOME.url, key: 'C'.repeat(43) };
    const again = paired(first.list, fresh, 'id-other', 30);
    expect(again.added).toBe(false);
    expect(again.list).toHaveLength(1);
    expect(again.device.id).toBe('id-home');
    expect(again.device.key).toBe(fresh.key);
  });
});

describe('the saved computers', () => {
  it('drop what is not a computer and fill what an older app did not save', () => {
    const saved = [{ id: 'a', url: HOME.url, key: HOME.key }, { id: 'b' }, null, 'x', { id: 'c', url: WORK.url, key: WORK.key, name: 'work', addedAt: 5, prefs: { bypass: true } }];
    expect(readDevices(saved)).toEqual([
      { id: 'a', url: HOME.url, key: HOME.key, name: 'home-pc.trycloudflare.com', addedAt: 0, prefs: { who: null, worktree: false, bypass: false } },
      { id: 'c', url: WORK.url, key: WORK.key, name: 'work', addedAt: 5, prefs: { who: null, worktree: false, bypass: true } },
    ]);
    expect(readDevices(null)).toEqual([]);
    expect(readDevices({})).toEqual([]);
  });
});

describe('pairing a computer again', () => {
  it('moves that computer to the new address and key, whatever changed', () => {
    const first = paired([], HOME, 'id-home', 10);
    const second = paired(first.list, WORK, 'id-work', 20);
    const fresh = { url: 'https://new-home.trycloudflare.com', key: 'D'.repeat(43) };
    const again = repaired(second.list, 'id-home', fresh)!;
    expect(again.list.map(device => [device.id, device.url, device.key])).toEqual([
      ['id-home', fresh.url, fresh.key],
      ['id-work', WORK.url, WORK.key],
    ]);
  });

  it('folds in a computer that already had that code', () => {
    const first = paired([], HOME, 'id-home', 10);
    const second = paired(first.list, WORK, 'id-work', 20);
    const again = repaired(second.list, 'id-home', WORK)!;
    expect(again.list.map(device => device.id)).toEqual(['id-home']);
    expect(again.device.key).toBe(WORK.key);
  });

  it('has nothing to move when the computer is gone', () => {
    expect(repaired([], 'id-home', HOME)).toBeNull();
  });
});
