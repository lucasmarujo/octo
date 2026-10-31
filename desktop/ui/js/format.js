export const SPINNER = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
export const USAGE_WARN = 70;
export const USAGE_ALERT = 90;

const LABELS = { antigravity: 'agy' };
const KIND_COLORS = {
  claude: '#de805c',
  codex: '#78afff',
  kilo: '#ece26e',
  opencode: '#fab283',
  antigravity: '#9696ff',
  grok: '#e8e8f0',
  qwen: '#ba8cff',
  kimi: '#ec80dc',
  terminal: '#5ad2d2',
};

export const COLUMNS = [
  { key: 'needs', title: 'Needs you', empty: 'No agent is waiting for you.' },
  { key: 'working', title: 'Working', empty: 'Nothing is running.' },
  { key: 'done', title: 'Done', empty: 'Finished agents land here.' },
  { key: 'idle', title: 'Idle', empty: 'No idle agents.' },
];

export const label = (kind) => LABELS[kind] || kind;
export const kindColor = (kind) => KIND_COLORS[kind] || '#9a9aa8';

const pad2 = (value) => String(value).padStart(2, '0');

export function duration(ms) {
  const secs = Math.floor(Math.max(0, ms) / 1000);
  if (secs < 60) return `${secs}s`;
  if (secs < 3_600) return `${Math.floor(secs / 60)}m${pad2(secs % 60)}s`;
  if (secs < 100 * 3_600) return `${Math.floor(secs / 3_600)}h${pad2(Math.floor((secs % 3_600) / 60))}m`;
  return `${Math.floor(secs / 86_400)}d${pad2(Math.floor((secs % 86_400) / 3_600))}h`;
}

export function alive(agent) {
  return Boolean(agent) && agent.state !== 'exited' && agent.state !== 'lost';
}

export function column(agent) {
  if (agent.starting) return 'working';
  if (agent.state === 'paused') return 'needs';
  if (agent.state === 'working') return 'working';
  if (agent.state === 'done') return 'done';
  return 'idle';
}

export function glyph(agent, frame) {
  if (agent.starting) return { text: '◌', tone: 't-faint' };
  if (agent.state === 'lost') return { text: '↻', tone: 't-red' };
  if (agent.state === 'exited') return { text: '✗', tone: 't-faint' };
  if (agent.kind === 'terminal') return { text: '$', tone: 't-text' };
  if (agent.state === 'working') return { text: SPINNER[frame % SPINNER.length], tone: 't-yellow', spin: true };
  if (agent.state === 'paused') return { text: '‖', tone: 't-yellow' };
  if (agent.state === 'done') return { text: '✓', tone: 't-green' };
  return { text: '●', tone: 't-faint' };
}

export function nameTone(agent) {
  if (!alive(agent)) return 't-faint';
  if (agent.kind === 'terminal') return 't-text';
  return { done: 't-text', working: 't-text', paused: 't-text' }[agent.state] || 't-muted';
}

export function elapsed(agent, now) {
  if (agent.startedAt === null || !alive(agent)) return null;
  if (agent.kind !== 'terminal') {
    if (agent.state === 'working') return { text: duration(now - (agent.turnStarted || agent.stateSince)), tone: 't-muted' };
    if (agent.state === 'paused') return { text: duration(now - agent.stateSince), tone: 't-yellow' };
    if (agent.lastTurn !== null) return { text: duration(now - agent.stateSince), tone: 't-muted' };
  }
  return { text: duration(now - agent.startedAt), tone: 't-muted' };
}

export function activity(agent) {
  if (agent.starting) return { text: 'starting…', tone: 't-faint' };
  if (agent.state === 'lost') return { text: 'Interrupted by a crash or a reboot', tone: 't-red' };
  if (agent.state === 'exited') return { text: agent.exit || 'ended', tone: 't-faint' };
  return { text: agent.activity, tone: agent.state === 'idle' ? 't-muted' : 't-text' };
}

export function usageColor(used) {
  if (used >= USAGE_ALERT) return 'var(--color-lost)';
  if (used >= USAGE_WARN) return 'var(--color-waiting)';
  return 'var(--color-done)';
}

export function plural(count, one, many) {
  return `${count} ${count === 1 ? one : many}`;
}
