export const HOST = 'lucas@studio';
export const FALLBACK_VERSION = '0.2.46';
export const MAIN_SESSION = 'launch-week';
export const SIDE_SESSION = 'api-v2';
export const THEME = 'gray';
export const VIEWS = ['board', 'list', 'grid', 'chat'];
export const AGENT_STATES = ['paused', 'working', 'done', 'idle', 'exited'];
export const KINDS = ['claude', 'codex', 'kilo', 'opencode', 'antigravity', 'grok', 'qwen', 'kimi', 'pi', 'cursor', 'terminal'];
export const HOLD_MS = 20000;
export const REPLY_MS = 1400;
export const START_MS = 1500;

export const PREFERENCES = {
  'orkai.theme': THEME,
  'orkai.view': 'board',
  'orkai.session': MAIN_SESSION,
  'orkai.expanded': JSON.stringify([MAIN_SESSION]),
  'orkai.tabs': JSON.stringify(['overview', 'octo']),
  'octo.pinnedTabs': JSON.stringify(['overview', 'octo']),
  'octo.octoTab': 'on',
  'octo.orchestratorView': 'chat',
};

const HOME = '/home/lucas';
const TERMINAL = 'terminal';
const BRANCH_PREFIX = 'octo/';
const NAME_WORDS = 3;
const NAME_MAX_LEN = 28;
const SECOND = 1000;
const MINUTE = 60 * SECOND;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;
const PROJECT_PALETTE = ['#baa6ff', '#96e2aa', '#f682a0', '#89bafa', '#fab682', '#8ce2d6', '#f0de96', '#e8aaec'];
const FNV_OFFSET = 0xcbf29ce484222325n;
const FNV_PRIME = 0x100000001b3n;
const STOPWORDS = new Set(['a', 'an', 'the', 'to', 'of', 'and', 'or', 'for', 'in', 'on', 'at', 'with', 'by', 'from', 'into', 'is', 'it', 'this', 'that', 'please', 'pls', 'can', 'you']);
const SESSION_DIRS = { [MAIN_SESSION]: '~/dev/shop', [SIDE_SESSION]: '~/dev/api' };
const DEFAULT_COLOR = -1;
const BOLD = 1;
const DIM = 2;
const RED = 1;
const GREEN = 2;
const YELLOW = 3;
const BLUE = 4;
const MAGENTA = 5;
const CYAN = 6;
const GRAY = 8;
const SNAPSHOT_FRAME = 1;
const BYTE = 256;

const MODELS = {
  claude: [
    { id: 'claude-opus-4-5', name: 'Opus', description: 'Most capable for complex work' },
    { id: 'claude-sonnet-4-5', name: 'Sonnet', description: 'Best for everyday tasks' },
    { id: 'claude-haiku-4-5', name: 'Haiku', description: 'Fastest for quick answers' },
  ],
  codex: [
    { id: 'gpt-5-codex', name: 'gpt-5-codex', description: 'Optimized for coding' },
    { id: 'gpt-5', name: 'gpt-5', description: 'Broad reasoning' },
  ],
};

const SEED = [
  { name: 'sidebar-sessions', kind: 'claude', session: MAIN_SESSION, dir: 'dev/shop', state: 'paused', age: 48, alive: 540, activity: 'Do you want to make this edit to sidebar.js?', task: 'Group the sidebar agents by session', worktree: true, model: 'claude-sonnet-4-5' },
  {
    name: 'checkout-redesign',
    kind: 'claude',
    session: MAIN_SESSION,
    dir: 'dev/shop',
    state: 'working',
    age: 252,
    alive: 1310,
    activity: 'Rewriting the cart summary component',
    task: 'Redesign the checkout page and keep the tests green',
    worktree: true,
    model: 'claude-opus-4-5',
    changes: { files: 6, insertions: 184, deletions: 72 },
    subagents: [
      { role: 'explore', description: 'Map the cart components', activity: 'Read CartSummary.tsx', age: 96 },
      { role: 'test-runner', description: 'Run the checkout suite', activity: 'npm test -- checkout', age: 41 },
    ],
  },
  { name: 'physics-merge', kind: 'codex', session: MAIN_SESSION, dir: 'dev/engine', state: 'working', age: 664, alive: 2140, activity: 'Running cargo test, 212 passed', task: 'Merge the physics branch and fix what breaks', worktree: true, model: 'gpt-5-codex', changes: { files: 14, insertions: 420, deletions: 233 } },
  { name: 'api-rate-limit', kind: 'cursor', session: MAIN_SESSION, dir: 'dev/api', state: 'working', age: 150, alive: 760, activity: 'Adding the sliding window limiter', task: 'Add a rate limit to the public API', worktree: true },
  { name: 'docs-search', kind: 'opencode', session: MAIN_SESSION, dir: 'dev/docs', state: 'working', age: 371, alive: 905, activity: 'Indexing 340 markdown files', task: 'Add search to the docs site', worktree: false },
  { name: 'folder-icon-update', kind: 'claude', session: MAIN_SESSION, dir: 'dev/shop', state: 'done', age: 62, alive: 1480, activity: 'Done. 3 files changed', task: 'Update the folder icons', worktree: true, model: 'claude-sonnet-4-5', changes: { files: 3, insertions: 41, deletions: 12 }, test: 'passed' },
  { name: 'localhost-server', kind: TERMINAL, session: MAIN_SESSION, dir: 'dev/shop', state: 'idle', age: 1930, alive: 1930, activity: 'npm run dev', task: 'npm run dev', worktree: false },
  { name: 'pricing-copy', kind: 'kimi', session: MAIN_SESSION, dir: 'dev/site', state: 'idle', age: 1085, alive: 2600, activity: 'Tighten the copy of the pricing page', task: 'Tighten the copy of the pricing page', worktree: false },
  { name: 'auth-tokens', kind: 'codex', session: SIDE_SESSION, dir: 'dev/api', state: 'working', age: 204, alive: 1120, activity: 'Rotating the refresh tokens', task: 'Rotate refresh tokens on every use', worktree: true, model: 'gpt-5-codex' },
  { name: 'openapi-docs', kind: 'claude', session: SIDE_SESSION, dir: 'dev/api', state: 'done', age: 430, alive: 1900, activity: 'Done. 5 files changed', task: 'Generate the OpenAPI reference', worktree: true, model: 'claude-sonnet-4-5', changes: { files: 5, insertions: 310, deletions: 8 }, test: 'passed' },
];

const FILES = {
  'checkout-redesign': [
    { path: 'src/cart/CartSummary.tsx', insertions: 88, deletions: 41 },
    { path: 'src/cart/CartLine.tsx', insertions: 37, deletions: 19 },
    { path: 'src/checkout/Checkout.tsx', insertions: 29, deletions: 8 },
    { path: 'src/checkout/checkout.css', insertions: 18, deletions: 4 },
    { path: 'tests/checkout.test.tsx', insertions: 12, deletions: 0 },
  ],
  'folder-icon-update': [
    { path: 'src/icons/folder.svg', insertions: 14, deletions: 6 },
    { path: 'src/icons/folder-open.svg', insertions: 15, deletions: 6 },
    { path: 'src/sidebar/Tree.tsx', insertions: 12, deletions: 0 },
  ],
};

const DIFF = [
  'diff --git a/src/icons/folder.svg b/src/icons/folder.svg',
  '--- a/src/icons/folder.svg',
  '+++ b/src/icons/folder.svg',
  '@@ -1,4 +1,4 @@',
  '-<svg viewBox="0 0 16 16" fill="none">',
  '+<svg viewBox="0 0 16 16" fill="none" stroke-linejoin="round">',
  '   <path d="M2 4h4l1.5 1.5H14V12H2z" stroke="currentColor"/>',
  ' </svg>',
].join('\n');

const INSTRUCTIONS = [
  '# Rules for Octo',
  '',
  '- Generated files can be replaced without asking.',
  '- Never run a migration, a deploy or a force push without me.',
  '- Tests have to pass before an agent says it is done.',
].join('\n');

const FOLDERS = {
  [HOME]: ['dev'],
  [`${HOME}/dev`]: ['api', 'docs', 'engine', 'shop', 'site'],
};

const run = (text, color = DEFAULT_COLOR, flags = 0) => [text, color, DEFAULT_COLOR, flags];
const blank = () => [run(' ')];
const tool = (name, detail) => [run('● ', GREEN), run(name, DEFAULT_COLOR, BOLD), run(`(${detail})`)];
const result = (text) => [run(`  ⎿ ${text}`, GRAY)];
const output = (text, color = DEFAULT_COLOR) => [run(text, color)];

const BODIES = {
  'sidebar-sessions': [tool('Read', 'src/sidebar/sidebar.js'), result('Read 214 lines'), tool('Update', 'src/sidebar/sidebar.js'), result('Group the agents under the session they belong to')],
  'checkout-redesign': [tool('Update', 'src/cart/CartSummary.tsx'), result('Updated with 42 additions and 18 removals'), tool('Task', 'Run the checkout suite'), result('test-runner · npm test -- checkout'), tool('Update', 'src/cart/CartLine.tsx'), result('Updated with 12 additions and 7 removals')],
  'physics-merge': [output('$ cargo test --workspace', CYAN), output('   Compiling engine-physics v0.9.0'), output('    Finished test profile in 41.2s'), output('running 240 tests'), output('test collide::sweep ... ok', GREEN), output('test solver::stack_of_ten ... ok', GREEN), output('212 passed · 28 left', GRAY)],
  'api-rate-limit': [tool('Edit', 'src/middleware/limit.ts'), result('Sliding window, 120 requests a minute per key'), tool('Edit', 'migrations/0042_rate_limits.sql'), result('New table rate_limits')],
  'docs-search': [output('→ build the index', BLUE), output('  scanned 340 markdown files'), output('  tokens 182,904 · titles 1,126'), output('→ write public/search.json', BLUE)],
  'folder-icon-update': [tool('Update', 'src/icons/folder.svg'), result('Updated with 14 additions and 6 removals'), tool('Bash', 'npm test'), result('38 passed')],
  'localhost-server': [blank(), output('  VITE v5.4.2  ready in 412 ms', GREEN), blank(), output('  ➜  Local:   http://localhost:5173/'), output('  ➜  press h + enter to show help', GRAY), output('12:04:18 [vite] hmr update /src/cart/CartSummary.tsx', GRAY)],
  'pricing-copy': [output('The three plans now fit in one line each.'), output('Nothing else is queued.', GRAY)],
  'auth-tokens': [output('$ npm test -- auth', CYAN), output('✓ rotates the refresh token on use', GREEN), output('✓ rejects a token that was already used', GREEN), output('… 9 more', GRAY)],
  'openapi-docs': [tool('Write', 'docs/openapi.yaml'), result('Wrote 310 lines'), tool('Bash', 'npm run lint:openapi'), result('No problems found')],
};

function statusLines(agent) {
  if (agent.starting) return [output('starting…', GRAY)];
  if (agent.state === 'paused') {
    return [[run('╭ ', YELLOW), run(agent.activity, YELLOW, BOLD)], [run('│ ', YELLOW), run('❯ 1. Yes', DEFAULT_COLOR, BOLD)], [run('│ ', YELLOW), run('  2. No, and tell the agent what to do', GRAY)]];
  }
  if (agent.state === 'working') return [[run('✻ ', MAGENTA), run(`${agent.activity}… `, MAGENTA), run('(esc to interrupt)', GRAY)]];
  if (agent.state === 'done') return [[run('✓ ', GREEN), run(agent.activity)]];
  if (agent.state === 'exited') return [output(agent.exit || 'exited', RED)];
  return [[run('> ', GRAY), run('Write the next task', GRAY, DIM)]];
}

export function screenOf(agent) {
  const body = BODIES[agent.name] || [];
  if (agent.kind === TERMINAL) return [[run('$ ', CYAN), run(agent.task)], ...body];
  return [[run(`${agent.kind} `, DEFAULT_COLOR, BOLD), run(agent.cwd, GRAY)], blank(), [run('> ', GRAY), run(agent.task)], blank(), ...body, blank(), ...statusLines(agent)];
}

export function previewOf(agent, rows) {
  return screenOf(agent).slice(-Math.max(1, rows));
}

function sgr([text, color, , flags]) {
  const codes = [];
  if (flags & BOLD) codes.push(1);
  if (flags & DIM) codes.push(2);
  if (color >= 8) codes.push(82 + color);
  else if (color >= 0) codes.push(30 + color);
  return codes.length ? `\x1b[${codes.join(';')}m${text}\x1b[0m` : text;
}

export function snapshotOf(agent, size) {
  const lines = previewOf(agent, size.rows).map((line) => line.map(sgr).join(''));
  const bytes = new TextEncoder().encode(lines.join('\r\n'));
  return [SNAPSHOT_FRAME, size.rows % BYTE, Math.floor(size.rows / BYTE), size.cols % BYTE, Math.floor(size.cols / BYTE), ...bytes];
}

export function projectColor(label) {
  let hash = FNV_OFFSET;
  for (const byte of new TextEncoder().encode(label)) hash = BigInt.asUintN(64, (hash ^ BigInt(byte)) * FNV_PRIME);
  return PROJECT_PALETTE[Number(hash % BigInt(PROJECT_PALETTE.length))];
}

export function projectLabel(cwd) {
  const names = cwd.split(/[\\/]/).filter(Boolean);
  return names.slice(-2).join('/') || cwd;
}

export function agentName(kind, task, taken) {
  const words = task.toLowerCase().split(/[^a-z0-9]+/).filter((word) => word && !STOPWORDS.has(word)).slice(0, NAME_WORDS);
  let slug = '';
  for (const word of words) {
    const next = slug ? `${slug}-${word}` : word;
    if (next.length > NAME_MAX_LEN) break;
    slug = next;
  }
  const base = slug || (kind === TERMINAL ? 'term' : kind);
  const first = slug ? 2 : 1;
  if (slug && !taken.has(slug)) return slug;
  for (let number = first; ; number += 1) {
    if (!taken.has(`${base}-${number}`)) return `${base}-${number}`;
  }
}

function seedAgent(seed, id, now) {
  const since = now - seed.age * SECOND;
  const ai = seed.kind !== TERMINAL;
  return {
    id,
    session: seed.session,
    name: seed.name,
    kind: seed.kind,
    state: seed.state,
    starting: false,
    activity: seed.activity,
    task: seed.task,
    cwd: `${HOME}/${seed.dir}`,
    project: seed.dir,
    color: projectColor(seed.dir),
    startedAt: now - seed.alive * SECOND,
    stateSince: since,
    turnStarted: seed.state === 'working' ? since : null,
    lastTurn: ai && (seed.state === 'done' || seed.state === 'idle') ? since : null,
    branch: seed.worktree ? `${BRANCH_PREFIX}${seed.name}` : null,
    exit: null,
    changes: seed.changes || null,
    test: seed.test || null,
    subagents: (seed.subagents || []).map((sub) => ({ role: sub.role, description: sub.description, activity: sub.activity, startedAt: now - sub.age * SECOND })),
    model: seed.model || null,
  };
}

function handledEntry(seq, at, agent, question, summary, by) {
  return { seq, at, agent: agent.id, name: agent.name, kind: agent.kind, project: agent.project, question, summary, by, outcome: 'answered', mode: 'moderate' };
}

function initialOrchestrator(agents, now) {
  const named = (name) => agents.find((agent) => agent.name === name);
  const asking = named('sidebar-sessions');
  return {
    online: true,
    enabled: true,
    mode: 'moderate',
    projects: [],
    primary: 'claude',
    secondary: 'codex',
    primaryModel: null,
    secondaryModel: null,
    engines: KINDS.filter((kind) => kind !== TERMINAL).map((kind) => ({ kind, installed: true })),
    decisions: [
      {
        id: 1,
        agent: asking.id,
        name: asking.name,
        kind: asking.kind,
        session: asking.session,
        project: asking.project,
        since: asking.stateSince,
        status: 'deciding',
        question: asking.activity,
        context: ['Update(src/sidebar/sidebar.js)', 'Group the agents under the session they belong to'],
        tool: { name: 'Update', detail: 'src/sidebar/sidebar.js' },
        suggestion: null,
        tag: null,
        problem: null,
        ended: false,
      },
    ],
    handled: [
      handledEntry(3, now - 3 * MINUTE, named('checkout-redesign'), 'Overwrite the snapshot files?', 'Yes, the snapshots are generated by the tests', 'rule'),
      handledEntry(2, now - 9 * MINUTE, named('physics-merge'), 'Run cargo test for the whole workspace?', 'Yes, the task asks for green tests', 'octo'),
      handledEntry(1, now - 14 * MINUTE, named('folder-icon-update'), 'Replace both folder icons?', 'Yes, that is the task', 'octo'),
    ],
    handledToday: 3,
    handledByProject: [
      { path: 'dev/shop', count: 2 },
      { path: 'dev/engine', count: 1 },
    ],
    chat: [
      { at: now - 22 * MINUTE, role: 'you', text: "Ship the checkout redesign. Only stop me for what can't be undone.", actions: [], failed: false },
      {
        at: now - 22 * MINUTE + 4 * SECOND,
        role: 'octo',
        text: "On it. I started three agents in launch-week and I'm watching them.",
        actions: ['started claude · checkout-redesign, in its own worktree', 'started codex · physics-merge', 'started cursor · api-rate-limit'],
        failed: false,
      },
    ],
    thinking: false,
    problem: null,
  };
}

export function initialState(now) {
  const agents = SEED.map((seed, index) => seedAgent(seed, index + 1, now));
  return {
    agents,
    sessions: [MAIN_SESSION, SIDE_SESSION],
    orchestrator: initialOrchestrator(agents, now),
    instructions: {},
    nextId: agents.length + 1,
    nextDecision: 2,
    nextSeq: 4,
  };
}

function patchAgent(state, id, patch) {
  return { ...state, agents: state.agents.map((agent) => (agent.id === id ? { ...agent, ...patch } : agent)) };
}

function patchOrchestrator(state, patch) {
  return { ...state, orchestrator: { ...state.orchestrator, ...patch } };
}

function working(activity, now) {
  return { state: 'working', starting: false, activity, stateSince: now, turnStarted: now, exit: null };
}

function countHandled(counts, project) {
  const known = counts.some((entry) => entry.path === project);
  if (!known) return [...counts, { path: project, count: 1 }];
  return counts.map((entry) => (entry.path === project ? { ...entry, count: entry.count + 1 } : entry));
}

function settle(state, agent, question, summary, by, now) {
  const view = state.orchestrator;
  const entry = handledEntry(state.nextSeq, now, agent, question, summary, by);
  const next = patchOrchestrator(state, {
    decisions: view.decisions.filter((decision) => decision.agent !== agent.id),
    handled: [entry, ...view.handled],
    handledToday: view.handledToday + 1,
    handledByProject: countHandled(view.handledByProject, agent.project),
  });
  return { ...next, nextSeq: state.nextSeq + 1 };
}

function absolute(path) {
  if (!path || path === '~') return HOME;
  return path.startsWith('~/') ? `${HOME}${path.slice(1)}` : path;
}

function start(state, message) {
  const kind = KINDS.includes(message.kind) ? message.kind : TERMINAL;
  const session = state.sessions.includes(message.session) ? message.session : state.sessions[0] || MAIN_SESSION;
  const cwd = absolute(message.cwd);
  const task = String(message.task || '').trim();
  const name = agentName(kind, task, new Set(state.agents.map((agent) => agent.name)));
  const project = projectLabel(cwd);
  const agent = {
    id: state.nextId,
    session,
    name,
    kind,
    state: 'idle',
    starting: true,
    activity: task,
    task,
    cwd,
    project,
    color: projectColor(project),
    startedAt: null,
    stateSince: null,
    turnStarted: null,
    lastTurn: null,
    branch: message.worktree && kind !== TERMINAL ? `${BRANCH_PREFIX}${name}` : null,
    exit: null,
    changes: null,
    test: null,
    subagents: [],
    model: message.model || null,
  };
  const sessions = state.sessions.includes(session) ? state.sessions : [...state.sessions, session];
  return { ...state, agents: [...state.agents, agent], sessions, nextId: state.nextId + 1 };
}

export function started(state, id, now) {
  const agent = state.agents.find((entry) => entry.id === id);
  if (!agent || !agent.starting) return state;
  if (agent.kind === TERMINAL) return patchAgent(state, id, { starting: false, state: 'idle', startedAt: now, stateSince: now });
  return patchAgent(state, id, { ...working('Reading the code around the task', now), startedAt: now });
}

function answer(state, message, now) {
  const decision = state.orchestrator.decisions.find((entry) => entry.id === message.decision);
  if (!decision) return state;
  if (message.how === 'dismiss') return patchOrchestrator(state, { decisions: state.orchestrator.decisions.filter((entry) => entry.id !== decision.id) });
  const agent = state.agents.find((entry) => entry.id === decision.agent);
  if (!agent) return patchOrchestrator(state, { decisions: state.orchestrator.decisions.filter((entry) => entry.id !== decision.id) });
  const summary = message.text || (message.how === 'suggestion' && decision.suggestion ? decision.suggestion.summary : 'Allowed');
  return patchAgent(settle(state, agent, decision.question, summary, 'you', now), agent.id, working('Continuing with your answer', now));
}

function configure(state, message) {
  const view = state.orchestrator;
  const patch = {};
  for (const key of ['enabled', 'mode', 'primary', 'secondary', 'primaryModel', 'secondaryModel']) {
    if (Object.hasOwn(message, key)) patch[key] = message[key];
  }
  if (message.project) {
    const others = view.projects.filter((entry) => entry.path !== message.project.path);
    patch.projects = message.project.mode ? [...others, { path: message.project.path, mode: message.project.mode }] : others;
  }
  return patchOrchestrator(state, patch);
}

function renameSession(state, message) {
  const to = String(message.to || '').trim();
  if (!to || state.sessions.includes(to) || !state.sessions.includes(message.name)) return state;
  return {
    ...state,
    sessions: state.sessions.map((name) => (name === message.name ? to : name)),
    agents: state.agents.map((agent) => (agent.session === message.name ? { ...agent, session: to } : agent)),
  };
}

function removeAgent(state, id) {
  return patchOrchestrator({ ...state, agents: state.agents.filter((agent) => agent.id !== id) }, { decisions: state.orchestrator.decisions.filter((decision) => decision.agent !== id) });
}

const REDUCERS = {
  start,
  stop: (state, message, now) => patchAgent(removeDecisions(state, message.agent), message.agent, { state: 'exited', starting: false, exit: 'exited', stateSince: now, turnStarted: null, subagents: [] }),
  remove: (state, message) => removeAgent(state, message.agent),
  restart: (state, message, now) => patchAgent(state, message.agent, working('Picking up where it stopped', now)),
  rename: (state, message) => (String(message.name || '').trim() ? patchAgent(state, message.agent, { name: String(message.name).trim() }) : state),
  seen: (state, message) => {
    const agent = state.agents.find((entry) => entry.id === message.agent);
    return agent && agent.state === 'done' ? patchAgent(state, agent.id, { state: 'idle' }) : state;
  },
  session: (state, message) => (message.name && !state.sessions.includes(message.name) ? { ...state, sessions: [...state.sessions, message.name] } : state),
  renameSession,
  deleteSession: (state, message) => ({
    ...patchOrchestrator(state, { decisions: state.orchestrator.decisions.filter((decision) => decision.session !== message.name) }),
    sessions: state.sessions.filter((name) => name !== message.name),
    agents: state.agents.filter((agent) => agent.session !== message.name),
  }),
  moveAgent: (state, message) => (state.sessions.includes(message.session) ? patchAgent(state, message.agent, { session: message.session }) : state),
  merge: (state, message) => patchAgent(state, message.agent, { branch: null, changes: null, test: null }),
  orchestratorSet: configure,
  orchestratorAnswer: answer,
  orchestratorChat: (state, message, now) => patchOrchestrator(state, { chat: [...state.orchestrator.chat, { at: now, role: 'you', text: String(message.text || ''), actions: [], failed: false }], thinking: true }),
  orchestratorClearChat: (state) => patchOrchestrator(state, { chat: [], thinking: false }),
  orchestratorSaveInstructions: (state, message) => ({ ...state, instructions: { ...state.instructions, [message.project || '']: String(message.text || '') } }),
};

function removeDecisions(state, id) {
  return patchOrchestrator(state, { decisions: state.orchestrator.decisions.filter((decision) => decision.agent !== id) });
}

export function applyMessage(state, message, now) {
  const reducer = message && Object.hasOwn(REDUCERS, message.type) ? REDUCERS[message.type] : null;
  return reducer ? reducer(state, message, now) : state;
}

function byName(state, name) {
  return state.agents.find((agent) => agent.name === name) || null;
}

function ask(state, step, agent, now) {
  const decision = {
    id: state.nextDecision,
    agent: agent.id,
    name: agent.name,
    kind: agent.kind,
    session: agent.session,
    project: agent.project,
    since: now,
    status: 'needsYou',
    question: step.question,
    context: step.context || [],
    tool: step.tool || null,
    suggestion: step.suggestion || null,
    tag: step.tag || null,
    problem: null,
    ended: false,
  };
  const paused = patchAgent(state, agent.id, { state: 'paused', activity: step.question, stateSince: now, turnStarted: null });
  return { ...patchOrchestrator(paused, { decisions: [...paused.orchestrator.decisions, decision] }), nextDecision: state.nextDecision + 1 };
}

const STEP_REDUCERS = {
  answer: (state, step, now) => {
    const agent = byName(state, step.agent);
    return agent ? patchAgent(settle(state, agent, agent.activity, step.summary, step.by, now), agent.id, working(step.activity, now)) : state;
  },
  finish: (state, step, now) => {
    const agent = byName(state, step.agent);
    if (!agent) return state;
    return patchAgent(state, agent.id, { state: 'done', activity: step.activity, stateSince: now, turnStarted: null, lastTurn: now, subagents: [], changes: step.changes || agent.changes, test: step.test || agent.test });
  },
  launch: (state, step, now) => started(start(state, launchMessage(step), now), state.nextId, now),
  ask: (state, step, now) => {
    const agent = byName(state, step.agent);
    return agent ? ask(state, step, agent, now) : state;
  },
  chat: (state, step, now) => applyMessage(state, { type: 'orchestratorChat', text: step.text }, now),
  reply: (state, step, now) => patchOrchestrator(state, { chat: [...state.orchestrator.chat, { at: now, role: 'octo', text: step.text, actions: step.actions || [], failed: false }], thinking: false }),
  approve: (state, step, now) => {
    const decision = state.orchestrator.decisions.find((entry) => entry.name === step.agent);
    return decision ? applyMessage(state, { type: 'orchestratorAnswer', decision: decision.id, how: 'allow' }, now) : state;
  },
};

export function launchMessage(step) {
  return { type: 'start', kind: 'claude', session: MAIN_SESSION, cwd: SESSION_DIRS[MAIN_SESSION], task: step.task, worktree: true, bypass: false, remoteControl: false, model: null, attachments: [] };
}

export function applyStep(state, step, now) {
  return Object.hasOwn(STEP_REDUCERS, step.type) ? STEP_REDUCERS[step.type](state, step, now) : state;
}

export const INTRO = { mood: 'working', say: "Hi, I'm Kai. This is the real Octo, running a simulated session." };

export const SCRIPT = [
  { view: 'board', wait: 3200, ...INTRO },
  { view: 'board', wait: 3400, mood: 'working', say: 'sidebar-sessions asked what the task already answers. Octo replied for you.', step: { type: 'answer', agent: 'sidebar-sessions', by: 'octo', summary: 'Yes, the task asks for it', activity: 'Applying the edit to sidebar.js' } },
  { view: 'board', wait: 3200, mood: 'done', say: 'physics-merge is done and its tests pass. One less to watch.', step: { type: 'finish', agent: 'physics-merge', activity: 'Done. 14 files changed, 240 tests pass', test: 'passed' } },
  { view: 'list', wait: 3600, mood: 'idle', say: 'This is the list view: every agent, grouped by project.' },
  { view: 'list', wait: 3800, mood: 'working', say: 'New task? Type it, press enter, and a fresh agent starts on it.', step: { type: 'launch', task: 'fix the flaky checkout test' } },
  { view: 'grid', wait: 4200, mood: 'working', say: 'The grid shows every terminal at once, as it runs.' },
  {
    view: 'grid',
    wait: 3600,
    mood: 'needs',
    say: 'api-rate-limit needs you. It wants to run a migration.',
    step: {
      type: 'ask',
      agent: 'api-rate-limit',
      question: 'Run the migration on staging?',
      context: ['Bash(npm run migrate -- --env staging)', 'Applies 0042_rate_limits.sql to the staging database'],
      tool: { name: 'Bash', detail: 'npm run migrate -- --env staging' },
      tag: 'irreversible',
    },
  },
  { view: 'chat', wait: 3800, mood: 'idle', say: 'This is Octo, the orchestrator. Ask it anything, in plain words.' },
  { view: 'chat', wait: 2200, mood: 'working', say: 'You ask what is going on, and Octo reads every agent for you.', step: { type: 'chat', text: 'What is waiting on me?' } },
  {
    view: 'chat',
    wait: 4600,
    mood: 'needs',
    say: "A migration can't be undone, so this one is yours to decide.",
    step: {
      type: 'reply',
      text: "One thing. api-rate-limit wants to run a migration on staging. That can't be undone and your rules keep migrations for you, so I left it waiting. Everything else is moving.",
      actions: ['answered sidebar-sessions · yes, the task asks for it', 'left api-rate-limit waiting for you'],
    },
  },
  { view: 'chat', wait: 3200, mood: 'working', say: 'You approved it. Back to work.', step: { type: 'approve', agent: 'api-rate-limit' } },
  { view: 'board', wait: 3400, mood: 'idle', say: 'Back on the board, nothing is waiting for you.' },
  { view: 'board', wait: 4200, mood: 'done', say: "checkout-redesign is done. That's how a session goes.", step: { type: 'finish', agent: 'checkout-redesign', activity: 'Done. 9 files changed, tests pass', changes: { files: 9, insertions: 268, deletions: 97 }, test: 'passed' } },
];

export function finalState(now) {
  return SCRIPT.reduce((state, beat) => (beat.step ? applyStep(state, beat.step, now) : state), initialState(now));
}

export function stateMessage(state, now) {
  return {
    type: 'state',
    host: HOST,
    now,
    session: state.sessions[0] || MAIN_SESSION,
    sessions: state.sessions.map((name) => ({ name, agents: state.agents.filter((agent) => agent.session === name).length })),
    agents: state.agents,
    whereDir: SESSION_DIRS[MAIN_SESSION],
    dirs: Object.fromEntries(state.sessions.filter((name) => SESSION_DIRS[name]).map((name) => [name, SESSION_DIRS[name]])),
    worktree: true,
    kinds: KINDS,
  };
}

export function orchestratorMessage(state) {
  return { type: 'orchestrator', ...state.orchestrator };
}

export function usageMessage(now) {
  return {
    type: 'usage',
    entries: [
      { kind: 'claude', windows: [{ label: '5h', used: 62, resetsAt: now + 2 * HOUR + 14 * MINUTE }, { label: 'week', used: 38, resetsAt: now + 3 * DAY + 6 * HOUR }], credits: null, problem: null },
      { kind: 'codex', windows: [{ label: '5h', used: 24, resetsAt: now + 3 * HOUR + 40 * MINUTE }, { label: 'week', used: 76, resetsAt: now + 2 * DAY + 10 * HOUR }], credits: null, problem: null },
    ],
  };
}

export function statusReply(state) {
  const count = (name) => state.agents.filter((agent) => !agent.starting && agent.state === name).length;
  const waiting = count('paused');
  const needs = waiting === 0 ? 'Nothing is waiting for you.' : waiting === 1 ? '1 agent is waiting for you.' : `${waiting} agents are waiting for you.`;
  return {
    type: 'reply',
    text: `This is a simulated session, so I can only report. ${state.agents.length} agents, ${count('working')} working, ${count('done')} done. ${needs}`,
    actions: [],
  };
}

function detailsOf(state, id) {
  const agent = state.agents.find((entry) => entry.id === id);
  if (!agent) return [];
  const test = agent.test ? { state: agent.test, command: 'npm test', startedAt: agent.stateSince - 40 * SECOND, finishedAt: agent.stateSince, code: 0 } : null;
  return [{ type: 'details', agent: id, subagents: agent.subagents, changes: agent.branch ? FILES[agent.name] || [] : null, overlaps: [], test, commits: null }];
}

function foldersOf(message) {
  const path = absolute(message.path).replace(/\/+$/, '') || '/';
  const names = FOLDERS[path] || [];
  const parent = path === '/' ? null : path.slice(0, path.lastIndexOf('/')) || '/';
  return [
    {
      type: 'folders',
      seq: message.seq || 0,
      path,
      parent,
      dirs: names.map((name) => ({ name, path: `${path}/${name}`, git: path !== HOME })),
      repo: null,
      missing: false,
      truncated: false,
      places: message.places ? [{ kind: 'home', name: '', path: HOME }] : null,
    },
  ];
}

const REPLIES = {
  orchestrator: (state) => [orchestratorMessage(state)],
  peek: (state, message) => [
    {
      type: 'peek',
      previews: (message.agents || []).map((id) => state.agents.find((agent) => agent.id === id)).filter(Boolean).map((agent) => ({ agent: agent.id, lines: previewOf(agent, message.rows || 1) })),
    },
  ],
  details: (state, message) => detailsOf(state, message.agent),
  models: (state, message) => [{ type: 'models', kind: message.kind, models: MODELS[message.kind] || [] }],
  skills: (state, message) => [{ type: 'skills', kind: message.kind, cwd: message.cwd, skills: [] }],
  diff: (state, message) => {
    const agent = state.agents.find((entry) => entry.id === message.agent);
    return agent && agent.branch ? [{ type: 'diff', agent: agent.id, branch: agent.branch, text: DIFF, truncated: false }] : [];
  },
  merge: () => [{ type: 'notice', text: 'Merged in this simulated session. Nothing was touched.', tone: 'success' }],
  folders: (state, message) => foldersOf(message),
  folderSearch: (state, message) => [{ type: 'folderSearch', seq: message.seq || 0, root: message.root, query: message.query, hits: [], truncated: false }],
  makeFolder: (state, message) => [{ type: 'folderMade', seq: message.seq || 0, path: `${message.parent}/${message.name}`, problem: null }],
  orchestratorInstructions: (state, message) => [instructionsOf(state, message.project)],
  orchestratorSaveInstructions: (state, message) => [instructionsOf(state, message.project)],
  orchestratorActivity: (state) => [{ type: 'orchestratorActivity', items: state.orchestrator.handled, next: null }],
};

function instructionsOf(state, project) {
  const key = project || '';
  const text = Object.hasOwn(state.instructions, key) ? state.instructions[key] : project ? '' : INSTRUCTIONS;
  return { type: 'orchestratorInstructions', project: project || null, text, path: project ? `${HOME}/${project}/OCTO.md` : `${HOME}/.octo/OCTO.md` };
}

export function repliesTo(state, message) {
  return message && Object.hasOwn(REPLIES, message.type) ? REPLIES[message.type](state, message) : [];
}

const CONFIG = {
  shell: null,
  shell_args: [],
  claude_command: 'claude',
  claude_args: [],
  codex_command: 'codex',
  codex_args: [],
  kilo_command: 'kilo',
  kilo_args: [],
  opencode_command: 'opencode',
  opencode_args: [],
  agy_command: 'agy',
  agy_args: [],
  grok_command: 'grok',
  grok_args: [],
  qwen_command: 'qwen',
  qwen_args: [],
  kimi_command: 'kimi',
  kimi_args: [],
  pi_command: 'pi',
  pi_args: [],
  cursor_command: 'cursor-agent',
  cursor_args: [],
  notify: false,
  notify_sound: false,
  notify_volume: 0,
  notify_command: null,
  notify_args: [],
  pwa_tunnel: 'cloudflare',
  pwa_port: 7470,
  island: false,
};

const DEVICES = [
  { id: 'macbook-air', name: 'macbook-air', address: '', kind: 'link', phase: 'live', agents: 4, working: 2, waiting: 0 },
  { id: 'build-server', name: 'build-server', address: 'deploy@build-server', kind: 'ssh', phase: 'live', agents: 2, working: 1, waiting: 0 },
];

const PHONE = { agent: 'Octo/1.0.2 (iPhone; CPU iPhone OS 18_1 like Mac OS X)', model: 'iPhone', osVersion: '18.1', standalone: true };
const PREVIEW_ONLY = 'This is a preview of Octo. Devices cannot be added here.';

function statuses() {
  return KINDS.filter((kind) => kind !== TERMINAL).map((kind) => ({ kind, installed: true, install: null }));
}

function here(agents) {
  return {
    agents: agents.length,
    working: agents.filter((agent) => agent.starting || agent.state === 'working').length,
    waiting: agents.filter((agent) => !agent.starting && agent.state === 'paused').length,
  };
}

export function createHub({ now = Date.now, later = setTimeout, version = FALLBACK_VERSION } = {}) {
  const terminals = new Map();
  const bootedAt = now();
  let state = initialState(bootedAt);
  let channel = null;
  let config = CONFIG;
  let phone = true;
  let nextTerminal = 1;

  const push = (message) => {
    if (channel && typeof channel.onmessage === 'function') channel.onmessage(message);
  };

  const repaintTerminals = () => {
    for (const terminal of terminals.values()) {
      const agent = state.agents.find((entry) => entry.id === terminal.agent);
      if (!agent || typeof terminal.channel.onmessage !== 'function') continue;
      const frame = snapshotOf(agent, terminal.size);
      const signature = frame.join();
      if (signature === terminal.shown) continue;
      terminal.shown = signature;
      terminal.channel.onmessage(frame);
    }
  };

  const publish = () => {
    push(stateMessage(state, now()));
    push(orchestratorMessage(state));
    repaintTerminals();
  };

  const change = (next) => {
    if (next === state) return;
    state = next;
    publish();
  };

  const settings = () => ({ config, path: '~/.octo/config.json', agents: statuses(), defaultShell: 'bash', shellRule: 'your $SHELL, then bash', storeUpdates: false, islandAvailable: false });

  const pairing = () => ({
    enabled: phone,
    phase: phone ? 'ready' : null,
    tunnel: phone ? 'cloudflare' : null,
    error: null,
    phones: phone ? 1 : 0,
    devices: phone ? [{ ...PHONE, sinceMs: bootedAt - 42 * MINUTE }] : [],
    link: null,
    qr: null,
  });

  const receive = (message) => {
    const before = state;
    change(applyMessage(state, message, now()));
    for (const reply of repliesTo(state, message)) push(reply);
    if (message.type === 'start') {
      const id = before.nextId;
      later(() => change(started(state, id, now())), START_MS);
    }
    if (message.type === 'orchestratorChat' && state !== before) later(() => change(applyStep(state, statusReply(state), now())), REPLY_MS);
  };

  const openTerminal = ({ agent, rows, cols, channel: screen }) => {
    const session = nextTerminal;
    nextTerminal += 1;
    terminals.set(session, { agent, channel: screen, size: { rows, cols }, shown: '' });
    later(repaintTerminals, 0);
    return session;
  };

  const resizeTerminal = ({ session, rows, cols }) => {
    const terminal = terminals.get(session);
    if (!terminal) return null;
    terminal.size = { rows, cols };
    repaintTerminals();
    return null;
  };

  const nothing = () => null;

  const commands = {
    attach: (args) => {
      channel = args.channel;
      later(() => {
        push({ type: 'welcome', host: HOST, version });
        push(stateMessage(state, now()));
        push(usageMessage(now()));
      }, 0);
      return null;
    },
    send: (args) => {
      if (args.message && typeof args.message === 'object') receive(args.message);
      return null;
    },
    home_dir: () => HOME,
    update_ready: nothing,
    check_update: () => ({ latest: version, newer: false, ready: false }),
    restart_app: nothing,
    settings,
    save_settings: (args) => {
      config = { ...config, ...(args.config || {}), notify_sound: false };
      return settings();
    },
    recheck_agents: statuses,
    install_agent: nothing,
    play_chime: () => false,
    alert: nothing,
    agent_updates: () => [],
    agent_version: nothing,
    update_agent: nothing,
    commit_info: () => ({ files: [], more: 0, url: null }),
    open_link: nothing,
    paste_image: nothing,
    pick_files: () => [],
    pairing,
    set_phone: (args) => {
      phone = Boolean(args.on);
      return pairing();
    },
    renew_phone: pairing,
    devices: () => ({ here: here(state.agents), devices: DEVICES }),
    add_device: () => Promise.reject(PREVIEW_ONLY),
    add_server: () => Promise.reject(PREVIEW_ONLY),
    rename_device: nothing,
    forget_device: nothing,
    retry_device: nothing,
    unlock_device: nothing,
    term_open: openTerminal,
    term_input: nothing,
    term_resize: resizeTerminal,
    term_close: (args) => {
      terminals.delete(args.session);
      return null;
    },
    island_attach: nothing,
    island_notch: nothing,
    island_open: nothing,
    island_shape: nothing,
  };

  return {
    commands,
    invoke: (name, args) => Promise.resolve().then(() => (Object.hasOwn(commands, name) ? commands[name](args || {}) : null)),
    state: () => state,
    play: (step) => change(applyStep(state, step, now())),
    reset: () => change(initialState(now())),
  };
}
