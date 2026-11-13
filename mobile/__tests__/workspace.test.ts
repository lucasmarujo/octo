import type { DevicePrefs } from '../src/core/devices';
import type { Identity, Transport } from '../src/core/link';
import { DeviceLink } from '../src/core/link';
import { answerMessage } from '../src/core/octo';
import type { ActivityEntry, AgentView, BridgeMsg, Decision, OrchestratorView, PhoneMsg, Run } from '../src/core/protocol';
import type { Source } from '../src/core/uploads';
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
    queued: () => 0,
    close: () => {},
  };
  const link = new DeviceLink('home', 'https://h', 'A'.repeat(43), wire, IDENTITY);
  link.connect();
  link.onReady(session);
  sent.length = 0;
  const open = jest.fn((uri: string): Source => ({ read: async (offset: number, length: number) => new Uint8Array(Math.max(0, Math.min(length, (uri.endsWith('.pdf') ? 0 : 10) - offset))), close: () => {} }));
  const effects = { toast: jest.fn(), buzz: jest.fn(), diff: jest.fn(), prefs: jest.fn(() => prefs), reduced: jest.fn(() => true), open };
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

function welcome(version: string): BridgeMsg {
  return { type: 'welcome', host: 'lucas-desktop', version };
}

function decision(id: number, part: Partial<Decision> = {}): Decision {
  return { id, agent: 7, name: 'pwa-mapping', kind: 'claude', session: 'main', project: 'octo', since: 5, status: 'needsYou', question: 'Drop the table?', context: [], tool: null, suggestion: null, tag: null, problem: null, ...part };
}

function entry(seq: number): ActivityEntry {
  return { seq, at: seq, agent: 7, name: 'pwa-mapping', kind: 'claude', project: 'octo', question: 'Run it?', summary: `Allowed ${seq}`, by: 'rule', outcome: 'answered', mode: 'moderate' };
}

function orchestrator(part: Partial<OrchestratorView> = {}): BridgeMsg {
  return {
    type: 'orchestrator',
    online: true,
    enabled: true,
    mode: 'moderate',
    projects: [],
    primary: 'claude',
    secondary: null,
    primaryModel: null,
    secondaryModel: null,
    engines: [{ kind: 'claude', installed: true }],
    decisions: [],
    handled: [],
    handledToday: 0,
    handledByProject: [],
    chat: [],
    thinking: false,
    problem: null,
    ...part,
  };
}

async function settle(): Promise<void> {
  for (let turn = 0; turn < 20; turn += 1) await Promise.resolve();
}

describe('Octo in the workspace of a computer', () => {
  it('asks for the view once per connection, and never an Octo that would not understand', () => {
    const { sent, hear } = setup();
    hear(welcome('0.2.39'));
    hear(state([]));
    hear(orchestrator());
    hear(state([]));
    expect(sent.filter(message => message.type === 'orchestrator')).toEqual([{ type: 'orchestrator' }]);
    hear(welcome('0.2.39'));
    expect(sent.filter(message => message.type === 'orchestrator')).toHaveLength(2);
    const old = setup();
    old.hear(welcome('0.2.38'));
    old.hear(state([]));
    expect(old.sent.some(message => message.type.startsWith('orchestrator'))).toBe(false);
    expect(old.workspace.octo.capable()).toBe(false);
  });

  it('asks at once when the screen opens on a computer that is already connected', () => {
    const first = setup();
    first.hear(welcome('0.2.40'));
    const second = new Workspace(first.workspace.link, first.effects);
    expect(first.sent.filter(message => message.type === 'orchestrator')).toHaveLength(2);
    second.dispose();
  });

  it('keeps the view the computer broadcasts, and the very same one when nothing changed', () => {
    const { workspace, hear } = setup();
    hear(welcome('0.2.39'));
    expect(workspace.octo.view.get()).toBeNull();
    hear(orchestrator({ decisions: [decision(1)], handledToday: 3 }));
    const first = workspace.octo.view.get()!;
    expect(first).toMatchObject({ online: true, enabled: true, handledToday: 3 });
    hear(orchestrator({ decisions: [decision(1)], handledToday: 3 }));
    expect(workspace.octo.view.get()).toBe(first);
    hear(orchestrator({ decisions: [decision(1), decision(2)], handledToday: 3 }));
    expect(workspace.octo.view.get()!.decisions[0]).toBe(first.decisions[0]);
    hear(orchestrator({ online: false }));
    expect(workspace.octo.view.get()).toMatchObject({ online: false });
  });

  it('opens Octo on the tab it was left on, and comes back to it from an agent opened there', () => {
    const { workspace, sent, hear } = setup();
    hear(welcome('0.2.39'));
    hear(state([agent({ state: 'paused' })]));
    workspace.openOcto();
    expect(workspace.store.get()).toMatchObject({ view: 'octo', tab: 'chat' });
    workspace.showTab('decisions');
    workspace.openAgent(7);
    expect(workspace.store.get().view).toBe('agent');
    workspace.leaveAgent();
    expect(workspace.store.get()).toMatchObject({ view: 'octo', tab: 'decisions', agent: null });
    workspace.leaveOcto();
    expect(workspace.store.get()).toMatchObject({ view: 'overview', home: 'overview' });
    workspace.openAgent(7);
    workspace.leaveAgent();
    expect(workspace.store.get().view).toBe('overview');
    workspace.openAgent(7);
    workspace.openOcto('projects');
    expect(workspace.store.get()).toMatchObject({ view: 'octo', tab: 'projects', agent: null });
    expect(sent.filter(message => message.type === 'close')).toHaveLength(3);
    workspace.leaveOcto();
    workspace.leaveOcto();
    expect(workspace.store.get().view).toBe('overview');
  });

  it('sends each control of Octo as its own message', () => {
    const { workspace, sent, hear } = setup();
    hear(welcome('0.2.39'));
    hear(orchestrator());
    sent.length = 0;
    const { octo } = workspace;
    octo.setMode('bypass');
    octo.setProjectMode('octo', 'observe');
    octo.setProjectMode('octo', null);
    octo.setEnabled(false);
    octo.turnOn('claude', 'moderate');
    octo.setEngine('primary', 'codex');
    octo.setEngine('secondary', null);
    octo.answer(answerMessage(4, 'suggestion'));
    octo.answer(answerMessage(4, 'text', { text: 'keep it', remember: 'ask before dropping' }));
    octo.answer(answerMessage(4, 'keys', { keys: ['1'] }));
    octo.answer(answerMessage(4, 'allow'));
    octo.answer(answerMessage(4, 'dismiss'));
    octo.clearChat();
    expect(sent).toEqual([
      { type: 'orchestratorSet', mode: 'bypass' },
      { type: 'orchestratorSet', project: { path: 'octo', mode: 'observe' } },
      { type: 'orchestratorSet', project: { path: 'octo', mode: null } },
      { type: 'orchestratorSet', enabled: false },
      { type: 'orchestratorSet', enabled: true, primary: 'claude', mode: 'moderate' },
      { type: 'orchestratorSet', primary: 'codex' },
      { type: 'orchestratorSet', secondary: null },
      { type: 'orchestratorAnswer', decision: 4, how: 'suggestion' },
      { type: 'orchestratorAnswer', decision: 4, how: 'text', text: 'keep it', remember: 'ask before dropping' },
      { type: 'orchestratorAnswer', decision: 4, how: 'keys', keys: ['1'] },
      { type: 'orchestratorAnswer', decision: 4, how: 'allow' },
      { type: 'orchestratorAnswer', decision: 4, how: 'dismiss' },
      { type: 'orchestratorClearChat' },
    ]);
  });

  it('sends what is typed in Octo to Octo, one turn at a time, and keeps the text while it thinks', () => {
    const { workspace, sent, hear } = setup();
    hear(welcome('0.2.39'));
    hear(state([]));
    hear(orchestrator());
    workspace.openOcto();
    sent.length = 0;
    expect(workspace.submit(' what is stuck? ')).toBe(true);
    expect(sent).toEqual([{ type: 'orchestratorChat', text: 'what is stuck?' }]);
    hear(orchestrator({ thinking: true }));
    expect(workspace.submit('and now?')).toBe(false);
    expect(workspace.submit('  ')).toBe(false);
    hear(orchestrator({ online: false }));
    expect(workspace.submit('hello')).toBe(false);
    expect(sent).toHaveLength(1);
    const away = setup();
    away.hear(orchestrator());
    away.workspace.openOcto();
    expect(away.workspace.submit('is anyone there?')).toBe(false);
    expect(away.sent.some(message => message.type === 'orchestratorChat')).toBe(false);
  });

  it('reads the activity log one page at a time, never the same page twice', () => {
    const { workspace, sent, hear } = setup();
    hear(welcome('0.2.39'));
    hear(orchestrator({ handled: [entry(60), entry(59)] }));
    sent.length = 0;
    const { octo } = workspace;
    octo.older();
    expect(sent).toEqual([]);
    octo.openLog();
    octo.openLog();
    octo.older();
    expect(sent).toEqual([{ type: 'orchestratorActivity', before: null }]);
    expect(octo.log.get().loading).toBe(true);
    hear({ type: 'orchestratorActivity', items: [entry(60), entry(59), entry(58)], next: 58 });
    expect(octo.log.get()).toMatchObject({ next: 58, loading: false, asked: true });
    octo.openLog();
    octo.older();
    octo.older();
    expect(sent).toEqual([
      { type: 'orchestratorActivity', before: null },
      { type: 'orchestratorActivity', before: 58 },
    ]);
    hear({ type: 'orchestratorActivity', items: [entry(57), entry(58)], next: null });
    expect(octo.log.get().older.map(item => item.seq)).toEqual([60, 59, 58, 57]);
    octo.older();
    expect(sent).toHaveLength(2);
    hear(welcome('0.2.39'));
    expect(octo.log.get()).toEqual({ older: [], next: null, loading: true, asked: false });
    expect(sent[sent.length - 1]).toEqual({ type: 'orchestratorActivity', before: null });
    hear({ type: 'orchestratorActivity', items: [entry(60)], next: null });
    expect(octo.log.get()).toMatchObject({ loading: false, asked: true });
  });

  it('asks again for a page that was lost with the connection, and never for a log nobody opened', () => {
    const { workspace, sent, hear } = setup();
    hear(welcome('0.2.39'));
    hear(welcome('0.2.39'));
    expect(sent.some(message => message.type === 'orchestratorActivity')).toBe(false);
    expect(workspace.octo.log.get().loading).toBe(false);
    workspace.octo.openLog();
    sent.length = 0;
    hear(welcome('0.2.39'));
    expect(sent).toEqual([{ type: 'orchestrator' }, { type: 'orchestratorActivity', before: null }]);
    expect(workspace.octo.log.get()).toEqual({ older: [], next: null, loading: true, asked: false });
  });

  it('asks again for the instructions that never came, and says a file is saved only when the computer says so', () => {
    const { workspace, sent, hear } = setup();
    hear(welcome('0.2.39'));
    const { octo } = workspace;
    octo.readInstructions('acme/web');
    sent.length = 0;
    hear(welcome('0.2.39'));
    expect(sent).toEqual([{ type: 'orchestrator' }, { type: 'orchestratorInstructions', project: 'acme/web' }]);
    hear({ type: 'orchestratorInstructions', project: 'acme/web', text: '- old', path: '/h/p.md' });
    sent.length = 0;
    expect(octo.saveInstructions('acme/web', '- new')).toBe(true);
    expect(octo.file.get()).toMatchObject({ saving: true, file: { text: '- old' } });
    hear(welcome('0.2.39'));
    expect(octo.file.get()).toMatchObject({ saving: false, file: { text: '- old' } });
    expect(sent.filter(message => message.type === 'orchestratorInstructions')).toEqual([]);
    expect(octo.saveInstructions('acme/web', '- new')).toBe(true);
    hear({ type: 'orchestratorInstructions', project: 'acme/web', text: '- new', path: '/h/p.md' });
    expect(octo.file.get()).toEqual({ project: 'acme/web', saving: false, file: { project: 'acme/web', text: '- new', path: '/h/p.md' } });
    expect(octo.saveInstructions('other', '- x')).toBe(false);
    const away = setup();
    away.workspace.octo.readInstructions(null);
    away.hear({ type: 'orchestratorInstructions', project: null, text: '- old', path: '/h/OCTO.md' });
    away.sent.length = 0;
    expect(away.workspace.octo.saveInstructions(null, '- new')).toBe(false);
    expect(away.sent).toEqual([]);
    expect(away.workspace.octo.file.get()).toMatchObject({ saving: false, file: { text: '- old' } });
  });

  it('reads and saves the instructions of the file that is open, ignoring the answer about another', () => {
    const { workspace, sent, hear } = setup();
    hear(welcome('0.2.39'));
    sent.length = 0;
    const { octo } = workspace;
    octo.readInstructions('acme/web');
    expect(octo.file.get()).toEqual({ project: 'acme/web', file: null, saving: false });
    hear({ type: 'orchestratorInstructions', project: null, text: '- global', path: '/h/OCTO.md' });
    expect(octo.file.get()!.file).toBeNull();
    hear({ type: 'orchestratorInstructions', project: 'acme/web', text: '- be brief', path: '/h/projects/ab12.md' });
    expect(octo.file.get()).toEqual({ project: 'acme/web', saving: false, file: { project: 'acme/web', text: '- be brief', path: '/h/projects/ab12.md' } });
    octo.saveInstructions('acme/web', '- be brief\n- ask first');
    expect(octo.file.get()).toMatchObject({ saving: true, file: { text: '- be brief' } });
    octo.readInstructions(null);
    expect(sent).toEqual([
      { type: 'orchestratorInstructions', project: 'acme/web' },
      { type: 'orchestratorSaveInstructions', project: 'acme/web', text: '- be brief\n- ask first' },
      { type: 'orchestratorInstructions', project: null },
    ]);
    octo.closeInstructions();
    hear({ type: 'orchestratorInstructions', project: null, text: '- global', path: '/h/OCTO.md' });
    expect(octo.file.get()).toBeNull();
  });
});

describe('launching an agent with files from the phone', () => {
  it('uploads the files first, then starts the agent with the places the computer kept them', async () => {
    const { workspace, sent, hear, effects } = setup({ who: 'claude', worktree: true, bypass: false });
    hear(welcome('0.2.39'));
    hear(state([]));
    sent.length = 0;
    workspace.uploads.add([
      { uri: 'file:///cache/shot.png', name: 'shot.png', size: 10 },
      { uri: 'file:///cache/spec.pdf', name: 'spec.pdf', size: 0 },
    ]);
    const launched = workspace.launch(' fix what the screenshot shows ');
    await settle();
    expect(sent.map(message => message.type)).toEqual(['attach']);
    const first = sent[0] as Extract<PhoneMsg, { type: 'attach' }>;
    expect(first).toMatchObject({ name: 'shot.png', part: 0, last: true });
    expect(atob(first.data)).toHaveLength(10);
    hear({ type: 'attached', seq: first.seq, path: '/h/attachments/k1/shot.png', problem: null });
    await settle();
    const second = sent[1] as Extract<PhoneMsg, { type: 'attach' }>;
    expect(second).toMatchObject({ name: 'spec.pdf', part: 0, last: true, data: '' });
    expect(second.seq).not.toBe(first.seq);
    hear({ type: 'attached', seq: second.seq, path: '/h/attachments/k2/spec.pdf', problem: null });
    await expect(launched).resolves.toBe(true);
    expect(sent[2]).toEqual({ type: 'start', kind: 'claude', session: 'main', cwd: '/w', task: 'fix what the screenshot shows', worktree: true, bypass: false, attachments: ['/h/attachments/k1/shot.png', '/h/attachments/k2/spec.pdf'] });
    expect(workspace.uploads.store.get()).toEqual([]);
    expect(effects.open).toHaveBeenCalledTimes(2);
  });

  it('starts with the files alone, but still asks for a task when there is neither', async () => {
    const { workspace, sent, hear, effects } = setup();
    hear(welcome('0.2.39'));
    hear(state([]));
    sent.length = 0;
    workspace.uploads.add([{ uri: 'file:///cache/shot.png', name: 'shot.png', size: 10 }]);
    const launched = workspace.launch('   ');
    await settle();
    hear({ type: 'attached', seq: (sent[0] as Extract<PhoneMsg, { type: 'attach' }>).seq, path: '/k/shot.png', problem: null });
    await expect(launched).resolves.toBe(true);
    expect(sent[1]).toMatchObject({ type: 'start', task: '', attachments: ['/k/shot.png'] });
    expect(effects.toast).not.toHaveBeenCalled();
    expect(workspace.submit('  ')).toBe(false);
    expect(effects.toast).toHaveBeenCalledWith('Write the task first', 'info', { mark: { text: '✎', tile: 'idle' } });
    expect(sent.filter(message => message.type === 'start')).toHaveLength(1);
    workspace.submit('plain task');
    expect(sent[sent.length - 1]).toEqual({ type: 'start', kind: 'claude', session: 'main', cwd: '/w', task: 'plain task', worktree: false, bypass: false });
  });

  it('does not start anything when a file fails, and keeps the files and the reason', async () => {
    const { workspace, sent, hear } = setup();
    hear(welcome('0.2.39'));
    hear(state([]));
    sent.length = 0;
    workspace.uploads.add([{ uri: 'file:///cache/shot.png', name: 'shot.png', size: 10 }]);
    const launched = workspace.launch('look at this');
    await settle();
    hear({ type: 'attached', seq: (sent[0] as Extract<PhoneMsg, { type: 'attach' }>).seq, path: null, problem: 'the file is over 25 MiB' });
    await expect(launched).resolves.toBe(false);
    expect(sent.some(message => message.type === 'start')).toBe(false);
    expect(workspace.uploads.store.get()[0]).toMatchObject({ phase: 'failed', problem: 'the file is over 25 MiB' });
  });

  it('fails the upload in flight when the connection comes back fresh', async () => {
    const { workspace, sent, hear } = setup();
    hear(welcome('0.2.39'));
    hear(state([]));
    workspace.uploads.add([{ uri: 'file:///cache/shot.png', name: 'shot.png', size: 10 }]);
    const launched = workspace.launch('look at this');
    await settle();
    hear(welcome('0.2.39'));
    await expect(launched).resolves.toBe(false);
    expect(workspace.uploads.store.get()[0].phase).toBe('failed');
    expect(sent.some(message => message.type === 'start')).toBe(false);
  });
});
