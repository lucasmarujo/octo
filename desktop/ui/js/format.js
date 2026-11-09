import { hubText, t } from './i18n.js';

export const SPINNER = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
export const USAGE_WARN = 70;
export const USAGE_ALERT = 90;
export const USAGE_FOLDED = 2;
export const SIDEBAR_DEFAULT = 248;
export const SIDEBAR_MIN = 240;
export const SIDEBAR_MAX = 480;
export const DETAILS_DEFAULT = 280;
export const DETAILS_MIN = 220;
export const DETAILS_MAX = 560;
export const FLYOUT_GAP = 6;
export const FLYOUT_MARGIN = 8;

const LABELS = { antigravity: 'agy' };
const TERMINAL_KIND = 'terminal';
const TERMINAL_MARK = '$';
const KIND_COLORS = {
  claude: '#de805c',
  codex: '#78afff',
  kilo: '#ece26e',
  opencode: '#fab283',
  antigravity: '#9696ff',
  grok: '#e8e8f0',
  qwen: '#ba8cff',
  kimi: '#ec80dc',
  pi: '#aadc5a',
  terminal: '#5ad2d2',
};

export const COLUMNS = [
  { key: 'needs', title: t('Needs you'), empty: t('No agent is waiting for you.') },
  { key: 'working', title: t('Working'), empty: t('Nothing is running.') },
  { key: 'done', title: t('Done'), empty: t('Finished agents land here.') },
  { key: 'idle', title: t('Idle'), empty: t('No idle agents.') },
];

function panelWidth(value, fallback, min, max) {
  const width = value === null || value === '' ? Number.NaN : Number(value);
  if (!Number.isFinite(width)) return fallback;
  return Math.round(Math.min(max, Math.max(min, width)));
}

export function sidebarWidth(value) {
  return panelWidth(value, SIDEBAR_DEFAULT, SIDEBAR_MIN, SIDEBAR_MAX);
}

export function detailsWidth(value) {
  return panelWidth(value, DETAILS_DEFAULT, DETAILS_MIN, DETAILS_MAX);
}

export function chosenModel(models, id) {
  return (Array.isArray(models) && id && models.find((model) => model.id === id)) || null;
}

export function optionsTone({ worktree, remote, bypass }) {
  if (bypass) return 'danger';
  if (remote) return 'caution';
  return worktree ? 'default' : 'neutral';
}

export const RESTORED_VOLUME = 50;
export const soundOn = (config) => Boolean(config.notify && config.notify_sound && config.notify_volume > 0);
export const soundFlip = (config) => (soundOn(config) ? { notify_sound: false } : { notify: true, notify_sound: true, notify_volume: config.notify_volume || RESTORED_VOLUME });
export const volumeChanges = (config, volume) => (volume > 0 ? { notify: true, notify_sound: true, notify_volume: volume } : { notify_volume: 0 });

export function installKind(agent) {
  if (agent.kind !== 'terminal') return null;
  const match = /^install-([a-z]+)(?:-\d+)?$/.exec(agent.name);
  if (!match) return null;
  const kind = Object.keys(LABELS).find((key) => LABELS[key] === match[1]) || match[1];
  return kind !== 'terminal' && Object.hasOwn(KIND_COLORS, kind) ? kind : null;
}

export function versionAtLeast(version, wanted) {
  const parts = (text) => String(text || '').split('.').map(Number);
  const [have, need] = [parts(version), parts(wanted)];
  for (let index = 0; index < need.length; index++) {
    if ((have[index] || 0) !== need[index]) return (have[index] || 0) > need[index];
  }
  return Boolean(version);
}

export function canMove(agent, session) {
  return Boolean(agent && session) && !agent.starting && agent.session !== session;
}

export const OVERVIEW_TAB = 'overview';

export function arrangeTabs(tabs, pinned) {
  return [...tabs.filter((id) => pinned.has(id)), ...tabs.filter((id) => !pinned.has(id))];
}

export function tabRange(tabs, pinned, id) {
  const count = tabs.filter((tab) => pinned.has(tab)).length;
  return pinned.has(id) ? { low: 0, high: count - 1 } : { low: count, high: tabs.length - 1 };
}

export function moveTab(tabs, pinned, id, to) {
  const from = tabs.indexOf(id);
  if (from === -1) return tabs;
  const { low, high } = tabRange(tabs, pinned, id);
  const at = Math.min(Math.max(to, low), high);
  if (at === from) return tabs;
  const next = tabs.filter((tab) => tab !== id);
  next.splice(at, 0, id);
  return next;
}

export function tabAfterClosing(tabs, at) {
  if (tabs.includes(OVERVIEW_TAB) || !tabs.length) return OVERVIEW_TAB;
  return tabs[Math.min(at, tabs.length - 1)];
}

const GRID_RAISING_COLUMNS = new Set(['needs', 'done']);
export const GRID_SIZES = ['4', '6', '8', '10', '12', 'all'];
export const DEFAULT_GRID_SIZE = '6';
export const MAX_PREVIEW_ROWS = 40;
const GRID_SHAPES = { 4: [2, 2], 6: [3, 2], 8: [4, 2], 10: [5, 2], 12: [4, 3] };

export function gridShape(size) {
  const shape = GRID_SHAPES[size];
  return shape ? { cols: shape[0], rows: shape[1] } : null;
}

export function previewRows(height, lineHeight) {
  if (!height || !lineHeight) return MAX_PREVIEW_ROWS;
  return Math.min(MAX_PREVIEW_ROWS, Math.max(1, Math.floor(height / lineHeight)));
}

export const label = (kind) => LABELS[kind] || kind;
export const kindHex = (kind) => KIND_COLORS[kind] || '#9a9aa8';
export const kindColor = (kind) => `var(--kind-${Object.hasOwn(KIND_COLORS, kind) ? kind : 'other'})`;
export const kindMark = (kind) => (kind === TERMINAL_KIND ? TERMINAL_MARK : label(kind).charAt(0).toUpperCase());

export function shortModel(id) {
  if (!id) return '';
  const name = id.split('/').pop().replace(/\[.*\]$/, '').replace(/-\d{8}$/, '');
  const claude = name.match(/claude-[\d.-]*([a-z]+)/);
  return claude ? claude[1] : name.split('-').slice(0, 2).join('-');
}

export const agentTip = (agent) => [agent.name, label(agent.kind), agent.model].filter(Boolean).join(' · ');
export const agentsTip = (list) => list.map(agentTip).join('\n');

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

export function arrangeGrid(order, columns, agents) {
  const raised = agents
    .filter((agent) => {
      const was = columns.get(agent.id);
      const now = column(agent);
      return was !== undefined && was !== now && GRID_RAISING_COLUMNS.has(now);
    })
    .map((agent) => agent.id);
  const raisedIds = new Set(raised);
  const present = new Set(agents.map((agent) => agent.id));
  const kept = order.filter((id) => present.has(id) && !raisedIds.has(id));
  const placed = new Set([...raised, ...kept]);
  const fresh = agents.map((agent) => agent.id).filter((id) => !placed.has(id));
  return { order: [...raised, ...kept, ...fresh], columns: new Map(agents.map((agent) => [agent.id, column(agent)])) };
}

export function idleRunning(list) {
  return list.filter((agent) => column(agent) === 'idle' && alive(agent) && !agent.starting);
}

export function idleStopped(list) {
  return list.filter((agent) => column(agent) === 'idle' && agent.state === 'exited');
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
  if (agent.starting) return { text: t('starting…'), tone: 't-faint' };
  if (agent.state === 'lost') return { text: t('Interrupted by a crash or a reboot'), tone: 't-red' };
  if (agent.state === 'exited') return { text: agent.exit ? hubText(agent.exit) : t('ended'), tone: 't-faint' };
  return { text: agent.activity, tone: agent.state === 'idle' ? 't-muted' : 't-text' };
}

export function groupByProject(agents) {
  const groups = new Map();
  for (const agent of agents) {
    if (!groups.has(agent.project)) groups.set(agent.project, { project: agent.project, color: agent.color, agents: [] });
    groups.get(agent.project).agents.push(agent);
  }
  return [...groups.values()];
}

export function changeParts(changes) {
  if (!changes || !changes.files) return null;
  return [
    { text: plural(changes.files, t('file'), t('files')), tone: 't-muted' },
    { text: `+${changes.insertions}`, tone: 't-green' },
    { text: `−${changes.deletions}`, tone: 't-red' },
  ];
}

export function testMark(test) {
  return {
    passed: { text: `✓ ${t('test')}`, tone: 't-green' },
    failed: { text: `✗ ${t('test')}`, tone: 't-red' },
    running: { text: `⠋ ${t('test')}`, tone: 't-yellow' },
    untrusted: { text: `! ${t('test')}`, tone: 't-yellow' },
  }[test] || null;
}

export function subagentLine(sub, now) {
  return { role: sub.role, time: duration(now - sub.startedAt), text: [sub.description, sub.activity].filter(Boolean).join(' · ') };
}

export function usageColor(used) {
  if (used >= USAGE_ALERT) return 'var(--color-lost)';
  if (used >= USAGE_WARN) return 'var(--color-waiting)';
  return 'var(--color-done)';
}

export function usageOrder(entries, kinds) {
  const running = new Set(kinds);
  const measured = (entry) => entry.windows.length > 0 || (entry.credits !== null && entry.credits !== undefined);
  const rank = (entry) => (running.has(entry.kind) ? 0 : 2) + (measured(entry) ? 0 : 1);
  return [...entries].sort((a, b) => rank(a) - rank(b));
}

export function usageShown(entries, open) {
  return open ? entries : entries.slice(0, USAGE_FOLDED);
}

export function sessionMark(name) {
  const words = name.trim().split(/[\s._\-/]+/).filter(Boolean).map((word) => [...word]);
  const letters = words.length > 1 ? [words[0][0], words[1][0]] : (words[0] || []).slice(0, 2);
  return letters.join('').toUpperCase();
}

export function flyoutPlace(anchor, size, viewport) {
  const left = Math.min(anchor.right + FLYOUT_GAP, viewport.width - size.width - FLYOUT_MARGIN);
  const top = Math.min(anchor.top, viewport.height - size.height - FLYOUT_MARGIN);
  return { left: Math.max(FLYOUT_MARGIN, left), top: Math.max(FLYOUT_MARGIN, top) };
}

export function plural(count, one, many) {
  return `${count} ${count === 1 ? one : many}`;
}

export function splitArgs(text) {
  const args = [];
  let current = '';
  let quote = null;
  let started = false;
  for (const char of text) {
    if (quote) {
      if (char === quote) quote = null;
      else current += char;
    } else if (char === '"' || char === "'") {
      quote = char;
      started = true;
    } else if (/\s/.test(char)) {
      if (started) args.push(current);
      current = '';
      started = false;
    } else {
      current += char;
      started = true;
    }
  }
  if (started) args.push(current);
  return args;
}

export function joinArgs(args) {
  return args.map((arg) => (arg === '' || /[\s"']/.test(arg) ? `"${arg.replaceAll('"', "'")}"` : arg)).join(' ');
}
