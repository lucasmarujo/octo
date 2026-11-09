import model from './model';
import type { AgentKind, AgentView, TestState } from './protocol';

export type TileTone = 'starting' | 'lost' | 'lostFill' | 'idle' | 'needs' | 'terminal' | 'working' | 'done';
export type PillTone = 'idle' | 'lost' | 'needs' | 'working' | 'done';
export type TextTone = 'text' | 'muted' | 'faint' | 'yellow' | 'green' | 'red' | 'link';

export type Status = {
  text: string;
  tile: TileTone;
  pill: PillTone;
  word: string;
  spin: boolean;
};

export type Doing = {
  text: string;
  tone: TextTone;
};

export type Mark = {
  text: string;
  tone: TextTone;
};

const LABELS: Partial<Record<AgentKind, string>> = { antigravity: 'agy' };
const NAMES: Record<AgentKind, string> = {
  claude: 'Claude',
  codex: 'Codex',
  opencode: 'OpenCode',
  pi: 'Pi',
  kilo: 'Kilo',
  terminal: 'Terminal',
  antigravity: 'Antigravity',
  grok: 'Grok',
  qwen: 'Qwen',
  kimi: 'Kimi',
};
const TESTS: Record<TestState, Mark> = {
  passed: { text: '✓ tests', tone: 'green' },
  failed: { text: '✗ tests', tone: 'red' },
  running: { text: '⠋ tests', tone: 'yellow' },
  untrusted: { text: '! tests', tone: 'yellow' },
};

export function label(kind: AgentKind): string {
  return LABELS[kind] || kind;
}

export function title(kind: AgentKind): string {
  return NAMES[kind] || kind;
}

export function alive(agent: AgentView): boolean {
  return agent.state !== 'exited' && agent.state !== 'lost';
}

export function waiting(agent: AgentView): boolean {
  return agent.state === 'paused' && !agent.starting;
}

export function status(agent: AgentView): Status {
  if (agent.starting) return { text: '◌', tile: 'starting', pill: 'idle', word: 'starting', spin: false };
  if (agent.state === 'lost') return { text: '↻', tile: 'lost', pill: 'lost', word: 'interrupted', spin: false };
  if (agent.state === 'exited') return { text: '✗', tile: 'idle', pill: 'idle', word: 'exited', spin: false };
  if (agent.state === 'paused') return { text: '‖', tile: 'needs', pill: 'needs', word: 'needs you', spin: false };
  if (agent.kind === 'terminal') return { text: '$', tile: 'terminal', pill: 'idle', word: agent.state === 'working' ? 'running' : 'open', spin: false };
  if (agent.state === 'working') return { text: model.SPINNER[0], tile: 'working', pill: 'working', word: 'working', spin: true };
  if (agent.state === 'done') return { text: '✓', tile: 'done', pill: 'done', word: 'done', spin: false };
  return { text: '●', tile: 'idle', pill: 'idle', word: 'idle', spin: false };
}

export function elapsed(agent: AgentView, now: number): string | null {
  if (agent.startedAt === null || !alive(agent)) return null;
  if (agent.kind !== 'terminal') {
    if (agent.state === 'working') return model.duration(now - (agent.turnStarted || agent.stateSince || 0));
    if (agent.state === 'paused') return model.duration(now - (agent.stateSince || 0));
    if (agent.lastTurn !== null) return model.duration(now - (agent.stateSince || 0));
  }
  return model.duration(now - agent.startedAt);
}

export function doing(agent: AgentView): Doing {
  if (agent.starting) return { text: 'Starting…', tone: 'faint' };
  if (agent.state === 'lost') return { text: 'Interrupted · recover it above', tone: 'red' };
  if (agent.state === 'exited') return { text: agent.exit || 'exited', tone: 'faint' };
  if (agent.state === 'paused') return { text: 'Waiting for your answer', tone: 'yellow' };
  return { text: agent.activity || agent.task || '', tone: agent.state === 'working' ? 'text' : 'muted' };
}

export function testMark(test: TestState | null | undefined): Mark | null {
  return (test && TESTS[test]) || null;
}
