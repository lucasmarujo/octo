import { idleRunning, idleStopped, removeAllAsk, removeAsk, still, stopAllAsk, stopAsk } from '../src/core/agents';
import type { LinkState } from '../src/core/link';
import model from '../src/core/model';
import type { AgentView, Run, UsageEntry } from '../src/core/protocol';
import { ringValue } from '../src/screens/device/Rings';
import { shiftFor } from '../src/screens/device/Terminal';
import { glyphOf } from '../src/screens/Devices';
import { scanWindow } from '../src/screens/Scan';
import { budgeted } from '../src/sheets/Diff';
import { pathAt } from '../src/sheets/Where';
import { usageLine } from '../src/sheets/Who';
import { ANDROID_STAND_INS, termText } from '../src/theme/type';
import { covered } from '../src/ui/keyboard';
import { providerXml } from '../src/ui/ProviderMark';

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

describe('the provider of an agent', () => {
  const desktop = jest.requireActual<{ PROVIDER_ICONS: Record<string, string> }>('../../desktop/ui/js/icons.js').PROVIDER_ICONS;

  it('is drawn with the very logo the desktop shows', () => {
    for (const kind of ['claude', 'codex', 'kilo', 'opencode', 'antigravity', 'grok', 'qwen', 'kimi'] as const) {
      expect(providerXml(kind)).toBe(desktop[kind]);
      expect(providerXml(kind)).toContain('viewBox="0 0 24 24"');
    }
  });

  it('keeps the mark of the agents without a logo and of the terminal', () => {
    expect(providerXml('pi')).toBeNull();
    expect(providerXml('terminal')).toBeNull();
    expect(providerXml('toString')).toBeNull();
    expect(model.kindMark('pi')).toBe('π');
  });
});

describe('stopping and removing agents', () => {
  const one = (id: number, state: AgentView['state'], extra: Partial<AgentView> = {}): AgentView => ({ ...agent(state), id, name: `agent-${id}`, ...extra });

  it('asks before ending a running agent and before deleting a stopped one, in the words of the desktop', () => {
    expect(stopAsk(one(1, 'working'))).toEqual({ title: 'Stop agent-1?', text: 'The agent process ends. Its card stays here until you remove it.', action: 'Stop' });
    expect(removeAsk(one(2, 'exited'))).toEqual({ title: 'Remove agent-2?', text: 'The agent and its recorded output are deleted.', action: 'Remove' });
    expect(removeAsk(one(3, 'lost', { branch: 'octo/fix-login' })).text).toBe('The agent and its recorded output are deleted. Its worktree and the octo/fix-login branch are deleted too.');
  });

  it('stops the idle agents still running and removes the stopped ones, never the working or the interrupted', () => {
    const list = [one(1, 'idle'), one(2, 'exited'), one(3, 'lost'), one(4, 'working'), one(5, 'done'), one(6, 'idle', { starting: true }), one(7, 'idle', { branch: 'octo/b' })];
    expect(idleRunning(list).map(entry => entry.id)).toEqual([1, 7]);
    expect(idleStopped(list).map(entry => entry.id)).toEqual([2]);
    expect(stopAllAsk(idleRunning(list))).toEqual({ title: 'Stop 2 idle agents?', text: 'Their processes end. The cards stay in Idle until you remove them.', action: 'Stop all' });
    expect(removeAllAsk(idleStopped(list))).toEqual({ title: 'Remove 1 stopped agent?', text: 'The agents and their recorded output are deleted.', action: 'Remove all' });
    expect(removeAllAsk([one(8, 'exited', { branch: 'octo/c' })]).text).toBe('The agents and their recorded output are deleted. Their worktrees and branches are deleted too.');
  });

  it('leaves out of a confirmed batch the agents that changed while the question was open', () => {
    const asked = [one(1, 'idle'), one(7, 'idle')];
    const now = [one(1, 'working'), one(7, 'idle'), one(9, 'idle')];
    expect(still(asked, idleRunning(now)).map(entry => entry.id)).toEqual([7]);
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

describe('the terminal on Android', () => {
  it('draws the symbols of Claude that no Android font carries with ones it does, cell for cell', () => {
    const footer = '  ⏵⏵ accept edits on (shift+tab to cycle)';
    expect(termText(footer, ANDROID_STAND_INS)).toBe('  ▸▸ accept edits on (shift+tab to cycle)');
    expect(termText('⏺ Read(app.js)', ANDROID_STAND_INS)).toBe('● Read(app.js)');
    expect(termText('⏸ plan mode on', ANDROID_STAND_INS)).toBe('‖ plan mode on');
    expect(termText(footer, ANDROID_STAND_INS)).toHaveLength(footer.length);
  });

  it('leaves the text alone where the fonts carry them', () => {
    const line = '  ⎿  Read 412 lines ⏵⏵ ✻ ● ⠋';
    expect(termText(line, null)).toBe(line);
    expect(termText('  ⎿  Read 412 lines ✻ ⠋', ANDROID_STAND_INS)).toBe('  ⎿  Read 412 lines ✻ ⠋');
  });
});

describe('the keyboard', () => {
  it('covers the navigation bar too on Android, which React Native leaves out of its height', () => {
    expect(covered(290, 48, 'android')).toBe(338);
    expect(covered(290, 0, 'android')).toBe(290);
  });

  it('is measured from the bottom of the screen on the iPhone', () => {
    expect(covered(336, 34, 'ios')).toBe(336);
  });

  it('covers nothing while it is closed or floats away from the bottom', () => {
    expect(covered(0, 48, 'android')).toBe(0);
    expect(covered(-48, 48, 'android')).toBe(0);
    expect(covered(0, 34, 'ios')).toBe(0);
  });
});

describe('the terminal of an agent with the keyboard open', () => {
  const LINE = 14.4;
  const HEIGHT = 300;
  const blank: Run[] = [['', -1, -1, 0]];
  const text = (value: string): Run[] => [[value, -1, -1, 0]];
  const screen = (rows: Run[][], cursor: [number, number] | null, scroll = 0) => ({ rows, cols: 40, cursor, scroll });

  it('keeps a shell that printed little in sight from the top', () => {
    const rows = [text('Microsoft Windows'), blank, text('C:\\work>'), ...Array<Run[]>(37).fill(blank)];
    expect(shiftFor(screen(rows, [2, 8]), LINE, HEIGHT)).toBe(0);
  });

  it('follows an agent that filled its screen down to the last line', () => {
    const rows = Array.from({ length: 40 }, (_, index) => text(`line ${index}`));
    expect(shiftFor(screen(rows, null), LINE, HEIGHT)).toBe(Math.round(6 + 40 * LINE - HEIGHT));
  });

  it('keeps the cursor in sight below the last printed line, unless the phone reads the history', () => {
    const rows = [text('a'), ...Array<Run[]>(39).fill(blank)];
    expect(shiftFor(screen(rows, [30, 0]), LINE, HEIGHT)).toBe(Math.round(6 + 31 * LINE - HEIGHT));
    expect(shiftFor(screen(rows, [30, 0], 5), LINE, HEIGHT)).toBe(0);
  });

  it('counts a colored bar without words as printed', () => {
    const bar: Run[] = [['   ', -1, 4, 0]];
    const rows = [...Array<Run[]>(35).fill(blank), bar, ...Array<Run[]>(4).fill(blank)];
    expect(shiftFor(screen(rows, null), LINE, HEIGHT)).toBe(Math.round(6 + 36 * LINE - HEIGHT));
  });
});

describe('the camera window', () => {
  it('stays square, centered and above the words at the bottom', () => {
    expect(scanWindow(390, 844)).toEqual({ left: 45, top: (844 - 300) * 0.4, side: 300 });
    expect(scanWindow(320, 400)).toEqual({ left: 60, top: 80, side: 200 });
  });
});
