import type { LinkState } from '../src/core/link';
import model from '../src/core/model';
import type { AgentView, UsageEntry } from '../src/core/protocol';
import { ringValue } from '../src/screens/device/Rings';
import { glyphOf } from '../src/screens/Devices';
import { scanWindow } from '../src/screens/Scan';
import { budgeted } from '../src/sheets/Diff';
import { pathAt } from '../src/sheets/Where';
import { usageLine } from '../src/sheets/Who';

jest.mock('../modules/octo-scanner', () => ({ __esModule: true, default: { requestAccess: jest.fn() }, ScannerView: () => null }));
jest.mock('../modules/octo-link', () => ({ __esModule: true, default: { open: jest.fn(), send: jest.fn(), close: jest.fn(), addListener: jest.fn() } }));
jest.mock('../src/core/storage', () => ({ load: jest.fn(async () => null), save: jest.fn(async () => {}) }));

const NOW = 1_800_000_000_000;

function agent(state: AgentView['state']): AgentView {
  return { id: 1, session: 's', name: 'a', kind: 'claude', state, starting: false, activity: '', task: '', cwd: '/', project: 'p', color: '#fff', startedAt: 1, stateSince: 1, turnStarted: 1, lastTurn: null, branch: null, exit: null };
}

function link(phase: LinkState['phase'], states: AgentView['state'][] | null): LinkState {
  const overview = states && { host: 'pc', now: NOW, session: 's', sessions: [], agents: states.map(agent), whereDir: '/', dirs: {}, worktree: false, kinds: [] };
  return { phase, overview, usage: null, skew: 0 };
}

describe('the row of a computer', () => {
  it('shows the mood of its agents once it is live, like the mascot of the computer', () => {
    expect(glyphOf(link('live', ['working', 'paused']))).toMatchObject({ tone: 'needs', text: '‖' });
    expect(glyphOf(link('live', ['working', 'done']))).toMatchObject({ tone: 'working', spin: true });
    expect(glyphOf(link('live', ['done', 'idle']))).toMatchObject({ tone: 'done', text: '✓' });
    expect(glyphOf(link('live', ['lost']))).toMatchObject({ tone: 'lost', text: '↻' });
    expect(glyphOf(link('live', []))).toMatchObject({ tone: 'idle' });
  });

  it('waits while it connects and warns when the computer is out of reach', () => {
    expect(glyphOf(link('connecting', null))).toMatchObject({ tone: 'starting' });
    expect(glyphOf(link('live', null))).toMatchObject({ tone: 'starting' });
    for (const phase of ['offline', 'unpaired', 'replaced', 'outdated'] as const) expect(glyphOf(link(phase, ['working']))).toMatchObject({ tone: 'lost', text: '!' });
  });
});

describe('the plan usage', () => {
  const claude: UsageEntry = { kind: 'claude', windows: [{ label: '5h', used: 42.4, resetsAt: NOW + 3_600_000 }, { label: 'week', used: 76, resetsAt: null }], credits: null, problem: null };

  it('reads like the rings and the picker of the phone app', () => {
    expect(ringValue(claude, NOW)).toEqual({ text: '42 · 76%', tone: 'muted' });
    expect(usageLine(claude, NOW)).toEqual({ text: '42% of 5h', tone: 'muted' });
  });

  it('turns red with the time left once a window is nearly used up', () => {
    const codex: UsageEntry = { kind: 'codex', windows: [{ label: '5h', used: model.USAGE_ALERT + 3, resetsAt: NOW + 37 * 60_000 }], credits: null, problem: null };
    expect(ringValue(codex, NOW)).toEqual({ text: '93% · 37m', tone: 'red' });
    expect(usageLine(codex, NOW)).toEqual({ text: '93% · resets 37m', tone: 'red' });
  });

  it('shows funds or the reason there is no limit', () => {
    expect(ringValue({ kind: 'opencode', windows: [], credits: 18.4, problem: null }, NOW)).toEqual({ text: 'funds', tone: 'muted' });
    expect(usageLine({ kind: 'opencode', windows: [], credits: 18.4, problem: null }, NOW)).toEqual({ text: '$18.40 in funds', tone: 'muted' });
    expect(ringValue({ kind: 'pi', windows: [], credits: null, problem: null }, NOW)).toEqual({ text: 'no limits', tone: 'faint' });
    expect(usageLine({ kind: 'pi', windows: [], credits: null, problem: 'signed out' }, NOW)).toEqual({ text: 'signed out', tone: 'faint' });
  });
});

describe('the folder picker', () => {
  it('climbs back up a path the way the computer writes it', () => {
    expect(pathAt('/Users/lucas/DEV/octo', 1)).toBe('/Users/lucas');
    expect(pathAt('C:\\Users\\lucas\\DEV', 0)).toBe('C:\\');
    expect(pathAt('C:\\Users\\lucas\\DEV', 2)).toBe('C:\\Users\\lucas');
  });
});

describe('the changes of an agent', () => {
  it('stop at the line budget so a huge diff never freezes the phone', () => {
    const lines = (count: number) => Array.from({ length: count }, (_, number) => ({ kind: 'add' as const, number, text: 'x' }));
    const files = [
      { name: 'a', insertions: 3, deletions: 0, hunks: [{ title: '', lines: lines(3) }] },
      { name: 'b', insertions: 5, deletions: 0, hunks: [{ title: '', lines: lines(5) }] },
      { name: 'c', insertions: 1, deletions: 0, hunks: [{ title: '', lines: lines(1) }] },
    ];
    const shown = budgeted(files, 6);
    expect(shown.files.map(file => [file.name, file.hunks[0].lines.length])).toEqual([
      ['a', 3],
      ['b', 3],
    ]);
    expect(shown.cut).toBe(true);
    expect(budgeted(files, 100).cut).toBe(false);
  });
});

describe('the camera window', () => {
  it('stays square, centered and above the words at the bottom', () => {
    expect(scanWindow(390, 844)).toEqual({ left: 45, top: (844 - 300) * 0.4, side: 300 });
    expect(scanWindow(320, 400)).toEqual({ left: 60, top: 80, side: 200 });
  });
});
