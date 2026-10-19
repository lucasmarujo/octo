export const SPINNER = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
export const WINDOW_TITLE = 'Orkai';
export const HINT = '↑↓ select · enter open · tab who · ^l where · ^n session · ^e rename · del delete · f1 help';
export const USAGE_WARN_PERCENT = 70;
export const USAGE_ALERT_PERCENT = 90;
export const USAGE_BAR_WIDTH = 12;
export const USAGE_REFRESH_MS = 120_000;
export const PROJECT_PALETTE_SIZE = 8;

const SECOND = 1_000;
const MINUTE = 60 * SECOND;
const HOUR = 60 * MINUTE;
const STARTING_MS = 900;

const STOPWORDS = new Set([
  'a', 'an', 'the', 'to', 'of', 'and', 'or', 'for', 'in', 'on', 'at', 'with', 'by', 'from', 'into', 'is', 'it',
  'this', 'that', 'please', 'pls', 'can', 'you', 'de', 'da', 'do', 'das', 'dos', 'o', 'os', 'as', 'e', 'um', 'uma',
  'para', 'pra', 'em', 'no', 'na', 'nos', 'nas', 'com', 'que', 'por', 'se', 'eu', 'voce', 'me', 'meu', 'minha',
  'favor',
]);

const FOLDS = { á: 'a', à: 'a', â: 'a', ã: 'a', ä: 'a', å: 'a', é: 'e', è: 'e', ê: 'e', ë: 'e', í: 'i', ì: 'i', î: 'i', ï: 'i', ó: 'o', ò: 'o', ô: 'o', õ: 'o', ö: 'o', ú: 'u', ù: 'u', û: 'u', ü: 'u', ç: 'c', ñ: 'n' };

const pad2 = (n) => String(n).padStart(2, '0');

export function durationShort(ms) {
  const secs = Math.floor(Math.max(0, ms) / SECOND);
  if (secs < 60) return `${secs}s`;
  if (secs < 3_600) return `${Math.floor(secs / 60)}m${pad2(secs % 60)}s`;
  if (secs < 100 * 3_600) return `${Math.floor(secs / 3_600)}h${pad2(Math.floor((secs % 3_600) / 60))}m`;
  return `${Math.floor(secs / 86_400)}d${pad2(Math.floor((secs % 86_400) / 3_600))}h`;
}

export function fnv1a64(text) {
  let hash = 0xcbf29ce484222325n;
  for (const byte of new TextEncoder().encode(text)) {
    hash ^= BigInt(byte);
    hash = (hash * 0x100000001b3n) & 0xffffffffffffffffn;
  }
  return hash;
}

export function projectColorIndex(label) {
  return Number(fnv1a64(label) % BigInt(PROJECT_PALETTE_SIZE));
}

export function usageTone(percent) {
  if (percent >= USAGE_ALERT_PERCENT) return 'red';
  if (percent >= USAGE_WARN_PERCENT) return 'yellow';
  return 'green';
}

export function usageCells(percent, cells) {
  const share = Math.min(Math.max(percent, 0), 100) / 100;
  return Math.min(Math.round(share * cells), cells);
}

export function slugify(text, maxWords = 3, maxLen = 28) {
  const words = text
    .split(/[^\p{L}\p{N}]+/u)
    .map((word) => [...word.toLowerCase()].map((c) => FOLDS[c] || c).filter((c) => /^[a-z0-9]$/.test(c)).join(''))
    .filter((word) => word && !STOPWORDS.has(word))
    .slice(0, maxWords);
  let slug = '';
  for (const word of words) {
    const extra = slug ? word.length + 1 : word.length;
    if (slug.length + extra > maxLen) {
      if (!slug) slug = word.slice(0, maxLen);
      break;
    }
    slug += slug ? `-${word}` : word;
  }
  return slug;
}

export function glyph(kind, state, frame) {
  if (kind === 'terminal') return ['$', 'text'];
  switch (state) {
    case 'working': return [SPINNER[frame % SPINNER.length], 'yellow'];
    case 'paused': return ['‖', 'yellow'];
    case 'done': return ['✔', 'green'];
    default: return ['●', 'dim'];
  }
}

function nameTone(kind, state) {
  if (kind === 'terminal') return 'text';
  return { done: 'green', working: 'text', paused: 'yellow' }[state] || 'muted';
}

export function createOverview(now) {
  const agent = (id, name, folder, kind, state, inState, life, activity) => ({
    id, name, folder, kind, state, activity, stateSince: now - inState, startedAt: now - life, startingUntil: 0, subagents: [],
  });
  return {
    machine: 'workstation',
    sessions: [{ name: 'default' }, { name: 'session-2' }, { name: 'session-3' }],
    agents: [
      agent(1, 'checkout-redesign', 'acme/web', 'claude', 'done', 32 * SECOND, 14 * MINUTE + 33 * SECOND, 'Running design update and committing changes'),
      agent(2, 'payment-tests', 'acme/web', 'codex', 'working', 64 * SECOND, 29 * MINUTE + 6 * SECOND, 'Writing integration tests for the payment flow'),
      agent(3, 'simulator-builds', 'acme/mobile', 'claude', 'working', 2 * MINUTE + 25 * SECOND, 43 * MINUTE + 39 * SECOND, 'Building and launching both apps in simulators'),
      agent(4, 'physics-merge', 'studio/game', 'opencode', 'paused', 3 * MINUTE + 46 * SECOND, HOUR + 41 * MINUTE, 'Resolve the conflict in solver.rs?'),
      agent(5, 'localhost-server', 'studio/game', 'terminal', 'working', 0, 58 * MINUTE + 12 * SECOND, 'npm run dev'),
      agent(6, 'container-ssh-access', 'lab/infra', 'agy', 'idle', 4 * MINUTE + 18 * SECOND, HOUR + 56 * MINUTE, 'Retrieving container SSH credentials'),
      agent(7, 'folder-icon-update', 'orkai/cli', 'kimi', 'done', 4 * MINUTE + 51 * SECOND, 2 * HOUR + 10 * MINUTE, 'Folder icon update'),
      agent(8, 'docker-install', 'orkai/cli', 'claude', 'working', 8 * MINUTE + 5 * SECOND, 2 * HOUR + 25 * MINUTE, 'Installing Docker inside the container'),
    ],
    usage: [
      { kind: 'claude', windows: [{ label: '5h', percent: 62, resetsAt: now + 2 * HOUR + 14 * MINUTE }, { label: 'week', percent: 23, resetsAt: now + 101 * HOUR }] },
      { kind: 'codex', windows: [{ label: '5h', percent: 12, resetsAt: now + 3 * HOUR + 41 * MINUTE }, { label: 'week', percent: 76, resetsAt: now + 58 * HOUR }] },
    ],
    usageUpdatedAt: now - 34 * SECOND,
    selected: 1,
    notice: null,
    nextId: 9,
    who: 'claude',
    where: '~/dev/acme/web',
  };
}

export const INTRO = { type: 'task', text: 'add rate limiting to the api', folder: 'acme/web', activity: 'Reading the api routes' };

export const CYCLE = [
  { type: 'state', id: 3, from: 'working', to: 'done' },
  { type: 'subagent', id: 8, role: 'explore', description: 'Find the docker install docs', activity: 'WebSearch' },
  { type: 'state', id: 2, from: 'working', to: 'paused', activity: 'Run the migrations now?' },
  { type: 'altn', id: 4, activity: 'Resolving the conflict in solver.rs' },
  { type: 'state', id: 9, from: 'working', to: 'done' },
  { type: 'subagent-done', id: 8 },
  { type: 'altn', id: 2, activity: 'Running the migrations' },
  { type: 'open', id: 3, from: 'done', activity: 'Rebuilding both apps after the fix' },
  { type: 'state', id: 4, from: 'working', to: 'paused', activity: 'Resolve the conflict in solver.rs?' },
  { type: 'state', id: 9, from: 'done', to: 'working', activity: 'Adding tests for the rate limiter' },
];

const isAi = (agent) => agent.kind !== 'terminal';

export function findAgent(overview, id) {
  const agent = overview.agents.find((a) => a.id === id);
  if (!agent) throw new Error(`agent ${id} not found`);
  return agent;
}

export function groups(overview) {
  const byFolder = new Map();
  for (const agent of overview.agents) {
    if (!byFolder.has(agent.folder)) byFolder.set(agent.folder, []);
    byFolder.get(agent.folder).push(agent);
  }
  return [...byFolder].map(([label, agents]) => ({ label, agents }));
}

export function stats(overview) {
  const result = { agents: 0, working: 0, paused: 0, done: 0 };
  for (const agent of overview.agents.filter(isAi)) {
    result.agents += 1;
    if (agent.state in result) result[agent.state] += 1;
  }
  return result;
}

export function windowTitle(overview) {
  const { paused, done } = stats(overview);
  const parts = [];
  if (paused > 0) parts.push(`‖${paused}`);
  if (done > 0) parts.push(`✔${done}`);
  return parts.length ? `${parts.join(' ')} · ${WINDOW_TITLE}` : WINDOW_TITLE;
}

export function oldestWaiting(overview) {
  const waiting = overview.agents.filter((a) => isAi(a) && a.state === 'paused');
  waiting.sort((a, b) => a.stateSince - b.stateSince);
  return waiting[0] || null;
}

function setState(agent, state, now, activity) {
  agent.state = state;
  agent.stateSince = now;
  if (activity) agent.activity = activity;
}

export function apply(overview, event, now) {
  const outcome = { bell: false, key: null, focus: null };
  switch (event.type) {
    case 'task': {
      const agent = {
        id: overview.nextId, name: slugify(event.text), folder: event.folder, kind: overview.who, state: 'working',
        activity: event.activity, stateSince: now, startedAt: now, startingUntil: now + STARTING_MS, subagents: [],
      };
      overview.nextId += 1;
      overview.agents.push(agent);
      overview.selected = agent.id;
      outcome.key = 'enter';
      outcome.focus = agent.id;
      break;
    }
    case 'state': {
      const agent = findAgent(overview, event.id);
      if (agent.state !== event.from) throw new Error(`agent ${event.id} is ${agent.state}, expected ${event.from}`);
      setState(agent, event.to, now, event.activity);
      if (event.to === 'paused') {
        overview.notice = { tone: 'warn', text: `${agent.name} is waiting for you · alt+n opens it` };
        outcome.bell = true;
      } else if (event.to === 'done') {
        overview.notice = { tone: 'success', text: `${agent.name} finished · alt+n opens it` };
        outcome.bell = true;
      }
      break;
    }
    case 'altn': {
      const agent = oldestWaiting(overview);
      if (!agent || agent.id !== event.id) throw new Error(`alt+n opens ${agent && agent.id}, expected ${event.id}`);
      setState(agent, 'working', now, event.activity);
      overview.selected = agent.id;
      overview.notice = null;
      outcome.key = 'alt+n';
      outcome.focus = agent.id;
      break;
    }
    case 'open': {
      const agent = findAgent(overview, event.id);
      if (agent.state !== event.from) throw new Error(`agent ${event.id} is ${agent.state}, expected ${event.from}`);
      setState(agent, 'working', now, event.activity);
      overview.selected = agent.id;
      overview.notice = null;
      outcome.key = 'enter';
      outcome.focus = agent.id;
      break;
    }
    case 'subagent':
      findAgent(overview, event.id).subagents.push({ role: event.role, description: event.description, activity: event.activity, startedAt: now });
      break;
    case 'subagent-done':
      findAgent(overview, event.id).subagents.shift();
      break;
    default:
      throw new Error(`unknown event ${event.type}`);
  }
  return outcome;
}

export function refreshUsage(overview, now) {
  if (now - overview.usageUpdatedAt < USAGE_REFRESH_MS) return false;
  overview.usageUpdatedAt = now;
  const session = overview.usage[0].windows[0];
  session.percent = Math.min(session.percent + 1, 99);
  return true;
}

export function lines(overview, now, frame) {
  const out = [];
  for (const group of groups(overview)) {
    out.push({ type: 'header', label: group.label, color: projectColorIndex(group.label), count: group.agents.length });
    for (const agent of group.agents) {
      const starting = now < agent.startingUntil;
      const [mark, markTone] = starting ? ['◌', 'dim'] : glyph(agent.kind, agent.state, frame);
      let time = '';
      let timeTone = 'dim';
      if (agent.kind !== 'terminal') {
        time = durationShort(now - agent.stateSince);
        timeTone = agent.state === 'working' || agent.state === 'paused' ? 'yellow' : 'dim';
      }
      let activity = agent.activity;
      let activityTone = { working: 'text', paused: 'yellow' }[agent.state] || 'muted';
      if (starting) {
        activity = 'starting…';
        activityTone = 'dim';
      } else if (agent.state === 'paused') {
        activity = `waiting for you · ${agent.activity}`;
      }
      out.push({
        type: 'agent', id: agent.id, selected: overview.selected === agent.id, spinning: !starting && agent.kind !== 'terminal' && agent.state === 'working',
        mark, markTone, name: agent.name, nameTone: nameTone(agent.kind, agent.state), time, timeTone,
        uptime: durationShort(now - agent.startedAt), kind: agent.kind, activity, activityTone,
      });
      for (const sub of agent.subagents) {
        out.push({ type: 'subagent', mark: SPINNER[frame % SPINNER.length], role: sub.role, time: durationShort(now - sub.startedAt), activity: `${sub.description} · ${sub.activity}` });
      }
    }
  }
  return out;
}

export function usageLines(overview, now) {
  return overview.usage.flatMap(({ kind, windows }) => windows.map((window, index) => ({
    kind: index === 0 ? kind : '',
    label: window.label,
    filled: usageCells(window.percent, USAGE_BAR_WIDTH),
    empty: USAGE_BAR_WIDTH - usageCells(window.percent, USAGE_BAR_WIDTH),
    percent: `${Math.round(window.percent)}%`,
    tone: usageTone(window.percent),
    resets: window.resetsAt > now ? `resets ${durationShort(window.resetsAt - now)}` : 'resets now',
  })));
}
