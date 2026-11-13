import {
  ACTIVITY_MAX,
  activityNote,
  activityPage,
  activityRequest,
  answerMessage,
  bypassAsk,
  byWord,
  canSaveInstructions,
  CHAT_CARDS,
  chatCards,
  chatMessage,
  chatRows,
  clearChatMessage,
  decisionItems,
  decisionMeta,
  deciding,
  enableBypassConfirm,
  enabledMessage,
  engineKinds,
  engineMessage,
  engineName,
  entryLine,
  fleetCounts,
  gateOf,
  headLine,
  INSTRUCTIONS_MAX_BYTES,
  installedEngines,
  instructionsFile,
  instructionsRequest,
  instructionsTooLong,
  mergeActivity,
  modeMessage,
  moreCards,
  needsBypassConfirm,
  needsCount,
  needsYou,
  octoCapable,
  octoMood,
  outcomeMark,
  projectBypassConfirm,
  projectModeMessage,
  projectRows,
  readView,
  RECENT_MS,
  sameRow,
  saveInstructionsMessage,
  saysText,
  secondaryChoices,
  segments,
  settled,
  shortAge,
  startChoice,
  suggestionAnswer,
  suggestionText,
  turnOnMessage,
  utf8Length,
  viewRequest,
} from '../src/core/octo';
import type { ActivityEntry, AgentView, Decision, OctoChatEntry, OrchestratorView, Suggestion } from '../src/core/protocol';

const NOW = 1_800_000_000_000;

function agent(id: number, project: string, state: AgentView['state'], part: Partial<AgentView> = {}): AgentView {
  return { id, session: 'main', name: `agent-${id}`, kind: 'claude', state, starting: false, activity: '', task: '', cwd: '/w', project, color: '#35fddc', startedAt: 1, stateSince: 1, turnStarted: 1, lastTurn: null, branch: null, exit: null, ...part };
}

function decision(id: number, part: Partial<Decision> = {}): Decision {
  return { id, agent: id + 10, name: `agent-${id + 10}`, kind: 'claude', session: 'main', project: 'acme/web', since: NOW - 60_000, status: 'needsYou', question: 'Drop the legacy table?', context: ['row one', 'row two'], tool: null, suggestion: null, tag: null, problem: null, ...part };
}

function handled(seq: number, part: Partial<ActivityEntry> = {}): ActivityEntry {
  return { seq, at: NOW - seq * 1000, agent: 3, name: 'physics-merge', kind: 'codex', project: 'studio/game', question: 'Run npm test?', summary: 'Allowed npm test', by: 'rule', outcome: 'answered', mode: 'moderate', ...part };
}

function said(role: OctoChatEntry['role'], at: number, text: string): OctoChatEntry {
  return { at, role, text, actions: [], failed: false };
}

function view(part: Partial<OrchestratorView> = {}): OrchestratorView {
  return {
    online: true,
    enabled: true,
    mode: 'moderate',
    projects: [],
    primary: 'claude',
    secondary: null,
    primaryModel: null,
    secondaryModel: null,
    engines: [
      { kind: 'claude', installed: true },
      { kind: 'codex', installed: true },
    ],
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

const ANSWER: Suggestion = { keys: null, text: 'No. Keep the table.', summary: 'Keep the table', reason: 'It cannot be undone.' };
const PRESS: Suggestion = { keys: ['1', 'Enter'], text: null, summary: 'Approve the plan', reason: 'It stays inside the task.' };

describe('the orchestrator on the phone', () => {
  it('only shows up for an Octo that has one', () => {
    expect(['0.2.39', '0.2.40', '0.3.0', '1.0.0'].map(octoCapable)).toEqual([true, true, true, true]);
    expect(['0.2.38', '0.2.36', '0.1.99', '', 'dev'].map(octoCapable)).toEqual([false, false, false, false, false]);
    expect(octoCapable(null)).toBe(false);
  });

  it('reads the view the computer sends, whatever is missing or malformed in it', () => {
    expect(readView(null)).toEqual(view({ enabled: false, primary: null, engines: [] }));
    expect(readView({ type: 'orchestrator', online: false, enabled: true, mode: 'bypass' })).toMatchObject({ online: false, enabled: true, mode: 'bypass' });
    const read = readView({
      enabled: true,
      mode: 'sideways',
      projects: [{ path: 'acme/web', mode: 'observe' }, { path: 'x', mode: 'nope' }, null],
      primary: '',
      secondary: 'codex',
      engines: [{ kind: 'claude', installed: true }, { kind: '', installed: true }, { kind: 'codex' }],
      decisions: [
        { id: 4, agent: 9, name: 'checkout', kind: 'claude', session: 'main', project: 'acme/web', since: 5, status: 'needsYou', question: 'Drop it?', context: ['a', 7, 'b'], tool: { name: 'Bash', detail: 'rm -rf dist' }, suggestion: { keys: ['1'], text: null, summary: 'Allow', reason: 'safe' }, tag: 'irreversible', problem: null },
        { id: 5, status: 'whatever', suggestion: { keys: [], text: '' } },
        { name: 'no id' },
      ],
      handled: [{ seq: 2, at: 9, agent: 1, name: 'a', kind: 'codex', project: 'p', question: 'q', summary: 's', by: 'robot', outcome: 'exploded', mode: 'bypass' }, { at: 1 }],
      handledToday: -3,
      handledByProject: [{ path: 'p', count: 4 }, { count: 2 }],
      chat: [{ at: 1, role: 'you', text: 'hi', actions: [], failed: false }, { at: 2, role: 'other', text: 'yo', actions: ['sent', '', 3], failed: 'yes' }, { role: 'octo' }],
      thinking: 'true',
      problem: 'claude is not signed in',
    });
    expect(read.online).toBe(true);
    expect(read.mode).toBe('moderate');
    expect(read.projects).toEqual([{ path: 'acme/web', mode: 'observe' }]);
    expect(read.primary).toBeNull();
    expect(read.engines).toEqual([
      { kind: 'claude', installed: true },
      { kind: 'codex', installed: false },
    ]);
    expect(read.decisions).toHaveLength(2);
    expect(read.decisions[0]).toMatchObject({ id: 4, context: ['a', 'b'], tool: { name: 'Bash', detail: 'rm -rf dist' }, suggestion: { keys: ['1'], text: null }, tag: 'irreversible' });
    expect(read.decisions[1]).toMatchObject({ id: 5, status: 'deciding', suggestion: null, question: '', context: [] });
    expect(read.handled).toEqual([{ seq: 2, at: 9, agent: 1, name: 'a', kind: 'codex', project: 'p', question: 'q', summary: 's', by: 'octo', outcome: 'failed', mode: 'bypass' }]);
    expect(read.handledToday).toBe(0);
    expect(read.handledByProject).toEqual([{ path: 'p', count: 4 }]);
    expect(read.chat).toEqual([said('you', 1, 'hi'), { at: 2, role: 'octo', text: 'yo', actions: ['sent'], failed: false }]);
    expect(read.thinking).toBe(false);
    expect(read.problem).toBe('claude is not signed in');
  });

  it('keeps the very same objects for what did not change, so only the rows that changed are drawn again', () => {
    const before = view({ decisions: [decision(1), decision(2)], handled: [handled(2), handled(1)], chat: [said('you', 1, 'hi')], projects: [{ path: 'a', mode: 'observe' }] });
    expect(settled(null, before)).toBe(before);
    const copy = JSON.parse(JSON.stringify(before)) as OrchestratorView;
    expect(settled(before, copy)).toBe(before);
    const next = JSON.parse(JSON.stringify(before)) as OrchestratorView;
    next.decisions[1].status = 'deciding';
    next.chat.push(said('octo', 2, 'hello'));
    next.thinking = true;
    const after = settled(before, next);
    expect(after).not.toBe(before);
    expect(after.decisions[0]).toBe(before.decisions[0]);
    expect(after.decisions[1]).not.toBe(before.decisions[1]);
    expect(after.handled).toBe(before.handled);
    expect(after.projects).toBe(before.projects);
    expect(after.engines).toBe(before.engines);
    expect(after.chat[0]).toBe(before.chat[0]);
    expect(after.chat).toHaveLength(2);
    const fewer = settled(after, { ...after, decisions: [after.decisions[1]] });
    expect(fewer.decisions).toEqual([after.decisions[1]]);
    expect(fewer.decisions[0]).toBe(after.decisions[1]);
  });

  it('counts what needs the user only while it is on, for the badge on the overview', () => {
    const busy = view({ decisions: [decision(1), decision(2, { status: 'deciding' }), decision(3)], handledToday: 212 });
    expect(needsYou(busy).map(entry => entry.id)).toEqual([1, 3]);
    expect(deciding(busy).map(entry => entry.id)).toEqual([2]);
    expect(needsCount(busy)).toBe(2);
    expect(needsCount({ ...busy, enabled: false })).toBe(0);
    expect(needsCount({ ...busy, online: false })).toBe(0);
    expect(needsCount(null)).toBe(0);
    expect(entryLine(busy)).toEqual({ text: 'moderate · 2 need you', needs: 2 });
    expect(entryLine(view({ decisions: [decision(1)], mode: 'bypass' }))).toEqual({ text: 'bypass · 1 needs you', needs: 1 });
    expect(entryLine(view({ handledToday: 14 }))).toEqual({ text: 'moderate · 14 handled today', needs: 0 });
    expect(entryLine(view({ enabled: false }))).toEqual({ text: 'Off · let it answer your agents', needs: 0 });
    expect(entryLine(view({ online: false, decisions: [decision(1)] }))).toEqual({ text: 'Open Octo on the computer', needs: 0 });
    expect(entryLine(null)).toEqual({ text: 'Asking the computer…', needs: 0 });
    expect(headLine(busy)).toBe('2 need you · 212 handled today');
    expect(headLine(view({ handledToday: 3 }))).toBe('3 handled today');
    expect(headLine(view({ online: false }))).toBe('Open Octo on the computer');
  });

  it('knows which screen to show: the intro while off, a calm wait while the computer is away, the last view when it had one', () => {
    expect(gateOf(view())).toBe('ready');
    expect(gateOf(view({ enabled: false }))).toBe('intro');
    expect(gateOf(view({ online: false, enabled: false }))).toBe('offline');
    expect(gateOf(view({ online: false }))).toBe('stale');
  });

  it('wears the mood of what is going on', () => {
    expect(octoMood(view({ decisions: [decision(1)] }))).toBe('needs');
    expect(octoMood(view({ decisions: [decision(1, { status: 'deciding' })] }))).toBe('working');
    expect(octoMood(view({ thinking: true }))).toBe('working');
    expect(octoMood(view())).toBe('idle');
    expect(octoMood(view({ online: false, decisions: [decision(1)] }))).toBe('idle');
    expect(octoMood(null)).toBe('idle');
  });
});

describe('the projects Octo looks after', () => {
  const agents = [
    agent(1, 'acme/web', 'working', { color: '#aa0000' }),
    agent(2, 'acme/web', 'paused'),
    agent(3, 'studio/game', 'done', { color: '#00aa00' }),
    agent(4, 'acme/web', 'idle'),
    agent(5, 'lab/infra', 'working'),
    agent(6, 'lab/infra', 'working', { starting: true }),
    agent(7, 'lab/search', 'exited'),
    agent(8, 'studio/game', 'working'),
    agent(9, 'studio/game', 'working'),
  ];
  const current = view({
    mode: 'moderate',
    projects: [
      { path: 'studio/game', mode: 'bypass' },
      { path: 'gone/project', mode: 'observe' },
    ],
    decisions: [decision(1, { project: 'acme/web', question: 'Drop orders_v1?' }), decision(2, { project: 'acme/web', question: 'Second one' }), decision(3, { project: 'lab/search', status: 'deciding' }), decision(4, { project: 'lab/infra', status: 'deciding' }), decision(5, { project: 'lab/infra', question: 'Restart prod?' })],
    handled: [handled(9, { project: 'studio/game', summary: 'Approved the merge plan' }), handled(8, { project: 'studio/game', summary: 'older' }), handled(7, { project: 'acme/web', summary: 'Allowed npm test' })],
    handledByProject: [
      { path: 'studio/game', count: 47 },
      { path: 'acme/web', count: 31 },
    ],
  });

  it('are the agents of the overview grouped by project, the ones waiting on the user first, then the busiest', () => {
    const rows = projectRows(agents, current);
    expect(rows.map(row => row.project)).toEqual(['acme/web', 'lab/infra', 'studio/game', 'lab/search']);
    expect(rows[0]).toEqual({
      project: 'acme/web',
      color: '#aa0000',
      names: ['agent-1', 'agent-2', 'agent-4'],
      counts: { total: 3, working: 1, needs: 1, done: 0, idle: 1 },
      needs: 2,
      says: { kind: 'needs', text: 'Drop orders_v1?' },
      handled: 31,
      mode: 'moderate',
      override: null,
    });
    expect(rows[2]).toMatchObject({ project: 'studio/game', color: '#00aa00', counts: { total: 3, working: 2, done: 1 }, needs: 0, says: { kind: 'handled', text: 'Approved the merge plan' }, handled: 47, mode: 'bypass', override: 'bypass' });
    expect(rows[1]).toMatchObject({ needs: 1, says: { kind: 'needs', text: 'Restart prod?' }, handled: 0 });
    expect(rows[3]).toMatchObject({ counts: { total: 1, idle: 1 }, says: { kind: 'deciding', text: '' } });
    expect(projectRows([], current)).toEqual([]);
    expect(projectRows(agents, view()).every(row => row.says.kind === 'none' && row.needs === 0 && row.mode === 'moderate')).toBe(true);
  });

  it('say in one line what Octo is doing in each of them', () => {
    const says = new Map(projectRows([...agents, agent(10, 'quiet/one', 'idle')], current).map(row => [row.project, saysText(row.says)]));
    expect(says.get('acme/web')).toBe('Waiting on you: Drop orders_v1?');
    expect(says.get('lab/search')).toBe('Deciding…');
    expect(says.get('studio/game')).toBe('Approved the merge plan');
    expect(says.get('quiet/one')).toBe('Nothing asked yet');
  });

  it('draw the state of their agents as shares of one bar, and count them by column', () => {
    expect(fleetCounts(agents)).toEqual({ total: 9, working: 5, needs: 1, done: 1, idle: 2 });
    expect(segments({ total: 4, working: 2, needs: 1, done: 0, idle: 1 })).toEqual([
      { column: 'working', share: 0.5 },
      { column: 'needs', share: 0.25 },
      { column: 'idle', share: 0.25 },
    ]);
    expect(segments({ total: 0, working: 0, needs: 0, done: 0, idle: 0 })).toEqual([]);
  });

  it('are the same row until something shown in it changes', () => {
    const [first] = projectRows(agents, current);
    const [again] = projectRows(agents, current);
    expect(again).not.toBe(first);
    expect(sameRow(first, again)).toBe(true);
    expect(sameRow(first, { ...again, handled: 32 })).toBe(false);
    expect(sameRow(first, { ...again, says: { kind: 'needs', text: 'Another' } })).toBe(false);
    expect(sameRow(first, { ...again, counts: { ...again.counts, working: 2 } })).toBe(false);
  });
});

describe('the engines Octo thinks with', () => {
  it('are the agents installed on the computer, the second never the same as the first', () => {
    const one = view({ engines: [{ kind: 'claude', installed: false }, { kind: 'codex', installed: true }], primary: 'codex' });
    expect(engineKinds(one)).toEqual(['claude', 'codex']);
    expect(engineKinds(view({ engines: [] }))).toEqual(['claude', 'codex']);
    expect(installedEngines(one)).toEqual(['codex']);
    expect(secondaryChoices(one)).toEqual([]);
    expect(secondaryChoices(view())).toEqual(['codex']);
    expect(startChoice(one, 'claude')).toBe('codex');
    expect(startChoice(view(), 'codex')).toBe('codex');
    expect(startChoice(view({ primary: null }), null)).toBe('claude');
    expect(startChoice(view({ engines: [] }), 'claude')).toBeNull();
    expect(['claude', 'codex', 'newcomer'].map(engineName)).toEqual(['Claude', 'Codex', 'newcomer']);
  });
});

describe('the decisions waiting in Octo', () => {
  it('list what needs the user, what Octo is judging and what it handled in the last ten minutes, with keys that never repeat', () => {
    const current = view({ decisions: [decision(1), decision(2, { status: 'deciding' }), decision(3)], handled: [handled(5, { at: NOW - 1000 }), handled(4, { at: NOW - RECENT_MS + 1 }), handled(3, { at: NOW - RECENT_MS })] });
    const items = decisionItems(current, NOW);
    expect(items.map(item => item.key)).toEqual(['head:needs', 'needs:1', 'needs:3', 'head:deciding', 'deciding:2', 'head:handled', 'handled:5', 'handled:4']);
    expect(items[0]).toEqual({ type: 'head', key: 'head:needs', text: 'Needs you', count: 2 });
    expect(items[5]).toMatchObject({ text: 'Handled in the last 10 minutes', count: 2 });
    expect(new Set(items.map(item => item.key)).size).toBe(items.length);
    expect(decisionItems(view(), NOW)).toEqual([{ type: 'empty', key: 'empty' }]);
    expect(decisionItems(view({ decisions: [decision(2, { status: 'deciding' })] }), NOW).map(item => item.type)).toEqual(['empty', 'head', 'deciding']);
  });

  it('show at most three cards inside the chat, and how many more wait in Decisions', () => {
    const many = view({ decisions: [1, 2, 3, 4, 5].map(id => decision(id)).concat(decision(6, { status: 'deciding' })) });
    expect(chatCards(many).map(entry => entry.id)).toEqual([1, 2, 3]);
    expect(chatCards(many)).toHaveLength(CHAT_CARDS);
    expect(moreCards(many)).toBe(2);
    expect(moreCards(view({ decisions: [decision(1)] }))).toBe(0);
  });

  it('describe a decision by its project and agent, and its suggestion by what would be sent', () => {
    expect(decisionMeta(decision(1, { tag: 'irreversible' }))).toBe('acme/web · claude');
    expect(decisionMeta(decision(1, { project: '', kind: 'codex' }))).toBe('codex');
    expect(suggestionText(ANSWER)).toBe('No. Keep the table.');
    expect(suggestionText(PRESS)).toBe('Approve the plan');
    expect(suggestionText({ ...PRESS, summary: '' })).toBe('1 Enter');
    expect(suggestionText(null)).toBe('');
  });

  it('age in one short unit', () => {
    expect([0, 12_000, 4 * 60_000 + 12_000, 3 * 3_600_000, -5].map(shortAge)).toEqual(['0s', '12s', '4m', '3h', '0s']);
  });
});

describe('what the phone tells Octo', () => {
  it('asks for the view, the activity log and the instructions', () => {
    expect(viewRequest()).toEqual({ type: 'orchestrator' });
    expect(activityRequest(null)).toEqual({ type: 'orchestratorActivity', before: null });
    expect(activityRequest(51)).toEqual({ type: 'orchestratorActivity', before: 51 });
    expect(activityRequest(Number.NaN)).toEqual({ type: 'orchestratorActivity', before: null });
    expect(instructionsRequest(null)).toEqual({ type: 'orchestratorInstructions', project: null });
    expect(instructionsRequest('')).toEqual({ type: 'orchestratorInstructions', project: null });
    expect(instructionsRequest('acme/web')).toEqual({ type: 'orchestratorInstructions', project: 'acme/web' });
    expect(saveInstructionsMessage(null, '- be brief')).toEqual({ type: 'orchestratorSaveInstructions', project: null, text: '- be brief' });
    expect(saveInstructionsMessage('acme/web', '')).toEqual({ type: 'orchestratorSaveInstructions', project: 'acme/web', text: '' });
  });

  it('changes one setting at a time, and nothing else', () => {
    expect(modeMessage('observe')).toEqual({ type: 'orchestratorSet', mode: 'observe' });
    expect(projectModeMessage('acme/web', 'bypass')).toEqual({ type: 'orchestratorSet', project: { path: 'acme/web', mode: 'bypass' } });
    expect(projectModeMessage('acme/web', null)).toEqual({ type: 'orchestratorSet', project: { path: 'acme/web', mode: null } });
    expect(enabledMessage(false)).toEqual({ type: 'orchestratorSet', enabled: false });
    expect(turnOnMessage('codex', 'moderate')).toEqual({ type: 'orchestratorSet', enabled: true, primary: 'codex', mode: 'moderate' });
    expect(engineMessage('primary', 'codex')).toEqual({ type: 'orchestratorSet', primary: 'codex' });
    expect(engineMessage('secondary', 'claude')).toEqual({ type: 'orchestratorSet', secondary: 'claude' });
    expect(engineMessage('secondary', null)).toEqual({ type: 'orchestratorSet', secondary: null });
    expect(JSON.parse(JSON.stringify(engineMessage('secondary', null)))).toEqual({ type: 'orchestratorSet', secondary: null });
  });

  it('answers a decision in each of the five ways', () => {
    expect(answerMessage(7, 'suggestion')).toEqual({ type: 'orchestratorAnswer', decision: 7, how: 'suggestion' });
    expect(answerMessage(7, 'text', { text: 'keep it' })).toEqual({ type: 'orchestratorAnswer', decision: 7, how: 'text', text: 'keep it' });
    expect(answerMessage(7, 'keys', { keys: ['2', 'Enter'] })).toEqual({ type: 'orchestratorAnswer', decision: 7, how: 'keys', keys: ['2', 'Enter'] });
    expect(answerMessage(7, 'allow')).toEqual({ type: 'orchestratorAnswer', decision: 7, how: 'allow' });
    expect(answerMessage(7, 'dismiss')).toEqual({ type: 'orchestratorAnswer', decision: 7, how: 'dismiss' });
    expect(answerMessage(7, 'suggestion', { text: 'ignored', keys: ['x'] })).toEqual({ type: 'orchestratorAnswer', decision: 7, how: 'suggestion' });
  });

  it('remembers a rule with any answer but a dismissal, and never an empty one', () => {
    expect(answerMessage(7, 'allow', { remember: '  never drop tables without asking ' })).toEqual({ type: 'orchestratorAnswer', decision: 7, how: 'allow', remember: 'never drop tables without asking' });
    expect(answerMessage(7, 'text', { text: 'no', remember: 'ask first' })).toMatchObject({ how: 'text', text: 'no', remember: 'ask first' });
    expect(answerMessage(7, 'suggestion', { remember: '   ' })).toEqual({ type: 'orchestratorAnswer', decision: 7, how: 'suggestion' });
    expect(answerMessage(7, 'dismiss', { remember: 'ask first' })).toEqual({ type: 'orchestratorAnswer', decision: 7, how: 'dismiss' });
  });

  it('sends the suggestion as it is, or the words of the user once they edit it', () => {
    const offered = decision(7, { suggestion: ANSWER });
    expect(suggestionAnswer(offered, 'No. Keep the table.', '')).toEqual({ type: 'orchestratorAnswer', decision: 7, how: 'suggestion' });
    expect(suggestionAnswer(offered, '  No. Keep the table. ', 'ask first')).toEqual({ type: 'orchestratorAnswer', decision: 7, how: 'suggestion', remember: 'ask first' });
    expect(suggestionAnswer(offered, '', '')).toEqual({ type: 'orchestratorAnswer', decision: 7, how: 'suggestion' });
    expect(suggestionAnswer(offered, null, '')).toEqual({ type: 'orchestratorAnswer', decision: 7, how: 'suggestion' });
    expect(suggestionAnswer(offered, 'Yes, drop it.', 'ok to drop')).toEqual({ type: 'orchestratorAnswer', decision: 7, how: 'text', text: 'Yes, drop it.', remember: 'ok to drop' });
    const keys = decision(8, { suggestion: PRESS });
    expect(suggestionAnswer(keys, 'Approve the plan', '')).toEqual({ type: 'orchestratorAnswer', decision: 8, how: 'suggestion' });
    expect(suggestionAnswer(keys, 'Reject it', '')).toEqual({ type: 'orchestratorAnswer', decision: 8, how: 'text', text: 'Reject it' });
    const bare = decision(9, { problem: 'claude is not signed in' });
    expect(suggestionAnswer(bare, '', '')).toBeNull();
    expect(suggestionAnswer(bare, 'go on', '')).toEqual({ type: 'orchestratorAnswer', decision: 9, how: 'text', text: 'go on' });
  });

  it('talks to Octo one turn at a time, only while it is on and reachable', () => {
    expect(chatMessage(view(), '  what is running?  ')).toEqual({ type: 'orchestratorChat', text: 'what is running?' });
    expect(chatMessage(view(), '   ')).toBeNull();
    expect(chatMessage(view({ thinking: true }), 'hello')).toBeNull();
    expect(chatMessage(view({ enabled: false }), 'hello')).toBeNull();
    expect(chatMessage(view({ online: false }), 'hello')).toBeNull();
    expect(chatMessage(null, 'hello')).toBeNull();
    expect(clearChatMessage()).toEqual({ type: 'orchestratorClearChat' });
  });

  it('asks before Bypass, saying plainly that nothing will wait', () => {
    expect(needsBypassConfirm('bypass', 'moderate')).toBe(true);
    expect(needsBypassConfirm('bypass', 'bypass')).toBe(false);
    expect(needsBypassConfirm('observe', 'bypass')).toBe(false);
    expect(needsBypassConfirm(null, 'moderate')).toBe(false);
    expect(projectBypassConfirm(view({ mode: 'bypass' }), 'observe', null)).toBe(true);
    expect(projectBypassConfirm(view({ mode: 'bypass' }), 'moderate', null)).toBe(true);
    expect(projectBypassConfirm(view({ mode: 'bypass' }), null, 'bypass')).toBe(false);
    expect(projectBypassConfirm(view({ mode: 'bypass' }), 'bypass', null)).toBe(false);
    expect(projectBypassConfirm(view(), 'observe', 'bypass')).toBe(true);
    expect(projectBypassConfirm(view(), 'bypass', null)).toBe(false);
    expect(projectBypassConfirm(view(), null, 'observe')).toBe(false);
    expect(enableBypassConfirm(view({ enabled: false, mode: 'bypass' }))).toBe(true);
    expect(enableBypassConfirm(view({ enabled: true, mode: 'bypass' }))).toBe(false);
    expect(enableBypassConfirm(view({ enabled: false, mode: 'moderate' }))).toBe(false);
    expect(bypassAsk(null)).toEqual({ title: 'Switch Octo to Bypass?', text: 'Octo will answer every question and allow everything on its own, with no block list and no limits. Nothing waits for you.', action: 'Use Bypass' });
    expect(bypassAsk('acme/web').title).toBe('Run acme/web in Bypass?');
  });
});

describe('the conversation with Octo', () => {
  it('keys every entry apart, even two said in the same millisecond', () => {
    const rows = chatRows([said('you', 5, 'a'), said('octo', 5, 'b'), said('octo', 5, 'c'), said('you', 6, 'd'), said('octo', 5, 'e')]);
    expect(rows.map(row => row.key)).toEqual(['you:5', 'octo:5', 'octo:5#1', 'you:6', 'octo:5#2']);
    expect(rows[2].entry.text).toBe('c');
    expect(chatRows([])).toEqual([]);
  });
});

describe('the activity of Octo', () => {
  it('reads newest first, each entry once, whatever page it came in', () => {
    const merged = mergeActivity([handled(9), handled(8)], [handled(8, { summary: 'stale copy' }), handled(7), handled(3)]);
    expect(merged.map(entry => entry.seq)).toEqual([9, 8, 7, 3]);
    expect(merged[1].summary).toBe('Allowed npm test');
    expect(mergeActivity([], [])).toEqual([]);
  });

  it('comes in pages, the last one saying there is nothing older', () => {
    expect(activityPage({ type: 'orchestratorActivity', items: [handled(51), { nope: true }], next: 51 })).toEqual({ items: [handled(51)], next: 51 });
    expect(activityPage({ items: [], next: null })).toEqual({ items: [], next: null });
    expect(activityPage(null)).toEqual({ items: [], next: null });
    expect(activityPage({ items: 'x', next: 'y' })).toEqual({ items: [], next: null });
    expect(ACTIVITY_MAX).toBeGreaterThanOrEqual(500);
  });

  it('marks how each question ended and who settled it', () => {
    expect(outcomeMark(handled(1))).toEqual({ text: '✓', tone: 'green' });
    expect(outcomeMark(handled(1, { outcome: 'denied' }))).toEqual({ text: '✕', tone: 'red' });
    expect(outcomeMark(handled(1, { outcome: 'escalated' }))).toEqual({ text: '‖', tone: 'yellow' });
    expect(outcomeMark(handled(1, { outcome: 'failed' }))).toEqual({ text: '!', tone: 'red' });
    expect([handled(1, { by: 'octo' }), handled(1, { by: 'rule' }), handled(1, { by: 'you' })].map(byWord)).toEqual(['judgment', 'rule', 'you']);
    expect(activityNote(handled(1, { mode: 'bypass', by: 'octo' }))).toBe('studio/game · bypass · judgment');
    expect(activityNote(handled(1, { project: '' }))).toBe('moderate · rule');
  });
});

describe('the instructions of Octo', () => {
  it('are read from the answer of the computer', () => {
    expect(instructionsFile({ type: 'orchestratorInstructions', project: null, text: '- be brief', path: 'C:\\octo\\OCTO.md' })).toEqual({ project: null, text: '- be brief', path: 'C:\\octo\\OCTO.md' });
    expect(instructionsFile({ project: 'acme/web', text: 7 })).toEqual({ project: 'acme/web', text: '', path: '' });
    expect(instructionsFile(null)).toEqual({ project: null, text: '', path: '' });
  });

  it('can be saved once changed, up to 64 KiB counted in bytes', () => {
    expect(utf8Length('abc')).toBe(3);
    expect(utf8Length('é€😀')).toBe(9);
    expect(instructionsTooLong('a'.repeat(INSTRUCTIONS_MAX_BYTES))).toBe(false);
    expect(instructionsTooLong('a'.repeat(INSTRUCTIONS_MAX_BYTES + 1))).toBe(true);
    expect(instructionsTooLong('é'.repeat(INSTRUCTIONS_MAX_BYTES / 2 + 1))).toBe(true);
    expect(canSaveInstructions('old', 'new')).toBe(true);
    expect(canSaveInstructions('same', 'same')).toBe(false);
    expect(canSaveInstructions(null, 'new')).toBe(false);
    expect(canSaveInstructions('old', 'a'.repeat(INSTRUCTIONS_MAX_BYTES + 1))).toBe(false);
  });
});
