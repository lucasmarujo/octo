import type { DevicePrefs } from '../src/core/devices';
import type { Identity, Transport } from '../src/core/link';
import { DeviceLink } from '../src/core/link';
import type { AgentView, BridgeMsg, PhoneMsg, Run } from '../src/core/protocol';
import type { Effects } from '../src/core/workspace';
import { Workspace } from '../src/core/workspace';

const IDENTITY: Identity = { agent: 'Octo/1.0.0', device: { type: 'device', model: 'Pixel 9', osVersion: '16', standalone: true, client: 'phone-1' } };
const PROMPT = [
  ' Edit src/pwa/web/app.js?',
  '',
  ' ❯ 1. Yes',
  '   2. Yes, allow all edits during this session (shift+tab)',
  '   3. No, and tell Claude what to do differently (esc)',
];

type Setup = {
  workspace: Workspace;
  sent: PhoneMsg[];
  hear: (message: BridgeMsg) => void;
  effects: jest.Mocked<Effects>;
  prefs: DevicePrefs;
};

function agent(part: Partial<AgentView>): AgentView {
  return {
    id: 7,
    session: 'main',
    name: 'pwa-mapping',
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

function setup(prefs: DevicePrefs = { who: null, worktree: false, bypass: false }): Setup {
  const sent: PhoneMsg[] = [];
  let session = '';
  const wire: Transport = {
    open: id => {
      session = id;
    },
    send: (_id, text) => sent.push(JSON.parse(text)),
    close: () => {},
  };
  const link = new DeviceLink('home', 'https://h', 'A'.repeat(43), wire, IDENTITY);
  link.connect();
  link.onReady(session);
  sent.length = 0;
  const effects = { toast: jest.fn(), buzz: jest.fn(), diff: jest.fn(), prefs: jest.fn(() => prefs), reduced: jest.fn(() => true) };
  const workspace = new Workspace(link, effects);
  return { workspace, sent, hear: message => link.onMessage(session, JSON.stringify(message)), effects, prefs };
}

function state(agents: AgentView[], kinds: AgentView['kind'][] = ['claude', 'codex', 'terminal']): BridgeMsg {
  return { type: 'state', host: 'lucas-desktop', now: Date.now(), session: 'main', sessions: [{ name: 'main', agents: agents.length }, { name: 'launch', agents: 0 }], agents, whereDir: '/w', dirs: { launch: '/videos' }, worktree: false, kinds };
}

function screen(lines: string[], scroll = 0): BridgeMsg {
  return { type: 'screen', agent: 7, rows: lines.length, cols: 60, full: true, cursor: [0, 0], scroll, lines: lines.map((text, index) => [index, [[text, -1, -1, 0] as Run]]) };
}

describe('the workspace of a computer', () => {
  it('starts on the session the computer is in and asks for a preview of who waits', () => {
    const { workspace, sent, hear } = setup();
    hear(state([agent({ state: 'paused', stateSince: 5 }), agent({ id: 8, state: 'paused', session: 'launch' })]));
    expect(workspace.store.get().filter).toBe('main');
    expect(sent).toEqual([{ type: 'peek', agents: [7], rows: 18 }]);
    hear(state([agent({ state: 'paused', stateSince: 5 })]));
    expect(sent).toHaveLength(1);
  });

  it('turns the preview of a waiting agent into its first answer', () => {
    const { workspace, hear } = setup();
    hear(state([agent({ state: 'paused' })]));
    hear({ type: 'peek', previews: [{ agent: 7, lines: PROMPT.map(text => [[text, -1, -1, 0] as Run]) }] });
    expect(workspace.store.get().previews.get(7)!.options[0]).toEqual({ key: '1', label: 'Yes', danger: false });
  });

  it('opens an agent at the size of the phone and resizes it only when it changes', () => {
    const { workspace, sent, hear } = setup();
    hear(state([agent({})]));
    sent.length = 0;
    workspace.openAgent(7);
    workspace.fit({ rows: 30, cols: 48 });
    workspace.fit({ rows: 30, cols: 48 });
    workspace.fit({ rows: 32, cols: 48 });
    expect(sent).toEqual([
      { type: 'open', agent: 7, rows: 30, cols: 48 },
      { type: 'size', rows: 32, cols: 48 },
    ]);
    workspace.leaveAgent();
    expect(sent[sent.length - 1]).toEqual({ type: 'close' });
  });

  it('shows the question of a waiting agent and answers it with one tap', () => {
    const { workspace, sent, hear, effects } = setup();
    hear(state([agent({ state: 'paused' })]));
    workspace.openAgent(7);
    workspace.fit({ rows: 5, cols: 60 });
    hear(screen(PROMPT));
    const approval = workspace.store.get().approval!;
    expect(approval.asked.question).toBe('Edit src/pwa/web/app.js?');
    expect(approval.asked.options.map(option => option.key)).toEqual(['1', '2', '3']);
    workspace.answer('1', approval.sig);
    expect(sent[sent.length - 1]).toEqual({ type: 'keys', data: '1' });
    expect(effects.buzz).toHaveBeenCalledWith('tap');
  });

  it('hides the question while the phone scrolls back through the history', () => {
    const { workspace, hear } = setup();
    hear(state([agent({ state: 'paused' })]));
    workspace.openAgent(7);
    hear(screen(PROMPT, 3));
    expect(workspace.store.get().approval).toBeNull();
  });

  it('answers from the overview once the screen still asks the same', () => {
    const { workspace, sent, hear, effects } = setup();
    hear(state([agent({ state: 'paused' })]));
    workspace.answerFromOverview(workspace.findAgent(7)!, { key: '1', label: 'Yes', danger: false });
    hear(screen(PROMPT));
    expect(sent[sent.length - 1]).toEqual({ type: 'keys', data: '1' });
    expect(effects.toast).not.toHaveBeenCalled();
  });

  it('does not answer from the overview when the question changed', () => {
    const { workspace, sent, hear, effects } = setup();
    hear(state([agent({ state: 'paused' })]));
    workspace.answerFromOverview(workspace.findAgent(7)!, { key: '1', label: 'Yes', danger: false });
    hear(screen(['Run rm -rf build?', ' 1. Approve', ' 2. Reject']));
    expect(sent.some(message => message.type === 'keys')).toBe(false);
    expect(effects.toast).toHaveBeenCalledWith('The question changed. Answer it on the screen.', 'warn');
  });

  it('starts an agent with the choices of this computer, and never a worktree for a terminal', () => {
    const { workspace, sent, hear } = setup({ who: 'codex', worktree: true, bypass: true });
    hear(state([]));
    workspace.pickSession('launch');
    expect(workspace.submit('fix the tests')).toBe(true);
    expect(sent[sent.length - 1]).toEqual({ type: 'start', kind: 'codex', session: 'launch', cwd: '/videos', task: 'fix the tests', worktree: true, bypass: true });
    const shell = setup({ who: 'terminal', worktree: true, bypass: true });
    shell.hear(state([]));
    expect(shell.workspace.submit('')).toBe(true);
    expect(shell.sent[shell.sent.length - 1]).toMatchObject({ kind: 'terminal', worktree: false, bypass: false });
  });

  it('asks for the task before starting an agent', () => {
    const { workspace, sent, hear, effects } = setup();
    hear(state([]));
    expect(workspace.submit('   ')).toBe(false);
    expect(sent).toEqual([]);
    expect(effects.toast).toHaveBeenCalledWith('Write the task first', 'info', { mark: { text: '✎', tile: 'idle' } });
  });

  it('falls back to Claude, then to the first agent the computer has', () => {
    const { workspace, hear } = setup({ who: 'kilo', worktree: false, bypass: false });
    hear(state([], ['codex', 'claude']));
    expect(workspace.currentWho()).toBe('claude');
    hear(state([], ['codex', 'terminal']));
    expect(workspace.currentWho()).toBe('codex');
  });

  it('only hands the folder picker the answer to the last question', () => {
    const { workspace, hear } = setup();
    const heard: string[] = [];
    workspace.watchFolders(news => heard.push(news.type === 'folders' ? news.path : news.query));
    workspace.browse('/a');
    workspace.browse('/b');
    hear({ type: 'folders', seq: 1, path: '/a', parent: null, dirs: [], repo: null, missing: false, truncated: false });
    hear({ type: 'folders', seq: 2, path: '/b', parent: null, dirs: [], repo: null, missing: false, truncated: false });
    expect(heard).toEqual(['/b']);
  });
});
