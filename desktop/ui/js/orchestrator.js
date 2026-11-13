import { column, duration, groupByProject, label, shortModel } from './format.js';
import { t } from './i18n.js';

export const MODES = ['observe', 'moderate', 'bypass'];
export const DEFAULT_MODE = 'moderate';
export const OCTO_VIEWS = ['chat', 'decisions', 'activity', 'projects', 'settings'];
export const ENGINE_KINDS = ['claude', 'codex'];
export const RECENT_MS = 600_000;
export const CHAT_CARDS = 3;
export const RAIL_NEEDS = 5;
export const RAIL_HANDLED = 4;
export const INSTRUCTIONS_MAX_BYTES = 65_536;

export const MODE_NAMES = { observe: t('Observe'), moderate: t('Moderate'), bypass: t('Bypass') };
export const MODE_WORDS = { observe: t('observe'), moderate: t('moderate'), bypass: t('bypass') };
export const MODE_TEXTS = {
  observe: t('Octo watches and reports. It never answers an agent. Every question comes to you, with what it would have said.'),
  moderate: t('Octo answers what your rules and the task already settle. Anything truly open, irreversible or outside the task waits for you.'),
  bypass: t('Octo decides everything and allows everything, with no limits. Nothing waits for you. You read what it did in Activity.'),
};
export const BY_WORDS = { octo: t('judgment'), rule: t('rule'), you: t('you') };

const OUTCOME_MARKS = {
  answered: { text: '✓', tone: 't-green' },
  denied: { text: '✕', tone: 't-red' },
  escalated: { text: '‖', tone: 't-yellow' },
  failed: { text: '!', tone: 't-red' },
};
const UNKNOWN_MARK = { text: '·', tone: 't-faint' };

const listOf = (value) => (Array.isArray(value) ? value : []);
const textOf = (value) => (typeof value === 'string' && value ? value : null);
const countOf = (value) => (Number.isFinite(value) && value > 0 ? value : 0);

function decisionOf(entry) {
  if (!entry || !Number.isFinite(entry.id)) return null;
  return {
    id: entry.id,
    agent: entry.agent,
    name: textOf(entry.name) || '',
    kind: textOf(entry.kind) || '',
    session: textOf(entry.session) || '',
    project: textOf(entry.project) || '',
    since: countOf(entry.since),
    status: entry.status === 'needsYou' ? 'needsYou' : 'deciding',
    question: textOf(entry.question) || '',
    context: listOf(entry.context).filter((row) => typeof row === 'string'),
    tool: entry.tool && textOf(entry.tool.name) ? { name: entry.tool.name, detail: textOf(entry.tool.detail) || '' } : null,
    suggestion: suggestionOf(entry.suggestion),
    tag: textOf(entry.tag),
    problem: textOf(entry.problem),
  };
}

function suggestionOf(entry) {
  if (!entry) return null;
  const keys = listOf(entry.keys).filter((key) => typeof key === 'string' && key);
  const text = textOf(entry.text);
  if (!keys.length && !text) return null;
  return { keys: text ? null : keys, text, summary: textOf(entry.summary) || '', reason: textOf(entry.reason) || '' };
}

function activityOf(entry) {
  if (!entry || !Number.isFinite(entry.seq)) return null;
  return {
    seq: entry.seq,
    at: countOf(entry.at),
    agent: entry.agent,
    name: textOf(entry.name) || '',
    kind: textOf(entry.kind) || '',
    project: textOf(entry.project) || '',
    question: textOf(entry.question) || '',
    summary: textOf(entry.summary) || '',
    by: textOf(entry.by) || '',
    outcome: textOf(entry.outcome) || '',
    mode: MODES.includes(entry.mode) ? entry.mode : DEFAULT_MODE,
  };
}

function chatOf(entry) {
  if (!entry || typeof entry.text !== 'string') return null;
  return { at: countOf(entry.at), role: entry.role === 'you' ? 'you' : 'octo', text: entry.text, actions: listOf(entry.actions).filter((line) => typeof line === 'string' && line), failed: entry.failed === true };
}

export function orchestratorView(message) {
  const view = message && typeof message === 'object' ? message : {};
  return {
    enabled: view.enabled === true,
    mode: MODES.includes(view.mode) ? view.mode : DEFAULT_MODE,
    projects: listOf(view.projects).filter((entry) => entry && typeof entry.path === 'string' && MODES.includes(entry.mode)).map((entry) => ({ path: entry.path, mode: entry.mode })),
    primary: textOf(view.primary),
    secondary: textOf(view.secondary),
    primaryModel: textOf(view.primaryModel),
    secondaryModel: textOf(view.secondaryModel),
    engines: listOf(view.engines).filter((engine) => engine && textOf(engine.kind)).map((engine) => ({ kind: engine.kind, installed: engine.installed === true })),
    decisions: listOf(view.decisions).map(decisionOf).filter(Boolean),
    handled: listOf(view.handled).map(activityOf).filter(Boolean),
    handledToday: countOf(view.handledToday),
    handledByProject: listOf(view.handledByProject).filter((entry) => entry && typeof entry.path === 'string').map((entry) => ({ path: entry.path, count: countOf(entry.count) })),
    chat: listOf(view.chat).map(chatOf).filter(Boolean),
    thinking: view.thinking === true,
    problem: textOf(view.problem),
  };
}

export const OFF_VIEW = orchestratorView(null);

export function octoView(name) {
  return OCTO_VIEWS.includes(name) ? name : OCTO_VIEWS[0];
}

export function needsYou(view) {
  return view.decisions.filter((decision) => decision.status === 'needsYou');
}

export function handledByOcto(view) {
  return view.handled.filter((entry) => entry.by !== 'you' && entry.outcome !== 'escalated');
}

export function deciding(view) {
  return view.decisions.filter((decision) => decision.status === 'deciding');
}

export function needsCount(view) {
  return view && view.enabled ? needsYou(view).length : 0;
}

export function octoStatus(view) {
  return view && view.enabled ? { mode: view.mode, handled: view.handledToday } : null;
}

export function fleetCounts(agents) {
  const counts = { total: agents.length, working: 0, needs: 0, done: 0, idle: 0 };
  for (const agent of agents) counts[column(agent)] += 1;
  return counts;
}

export function navCounts(view, agents) {
  const projects = new Set(agents.map((agent) => agent.project)).size;
  if (!view.enabled) return { mode: null, needs: 0, handled: 0, agents: agents.length, projects };
  return { mode: view.mode, needs: needsYou(view).length, handled: view.handledToday, agents: agents.length, projects };
}

export function engineKinds(view) {
  return view.engines.length ? view.engines.map((engine) => engine.kind) : ENGINE_KINDS;
}

export function installedEngines(view) {
  return view.engines.filter((engine) => engine.installed).map((engine) => engine.kind);
}

export function engineInstalled(view, kind) {
  return installedEngines(view).includes(kind);
}

export function startChoice(view, picked) {
  const installed = installedEngines(view);
  if (picked && installed.includes(picked)) return picked;
  if (view.primary && installed.includes(view.primary)) return view.primary;
  return installed[0] || null;
}

export function secondaryChoices(view) {
  return installedEngines(view).filter((kind) => kind !== view.primary);
}

function saysIndex(view) {
  const waiting = new Map();
  const judging = new Set();
  const last = new Map();
  for (const decision of view.decisions) {
    if (decision.status !== 'needsYou') judging.add(decision.project);
    else if (!waiting.has(decision.project)) waiting.set(decision.project, { question: decision.question, count: 1 });
    else waiting.get(decision.project).count += 1;
  }
  for (const entry of view.handled) if (!last.has(entry.project)) last.set(entry.project, entry.summary);
  return { waiting, judging, last };
}

function saysFrom(index, project) {
  if (index.waiting.has(project)) return { kind: 'needs', text: index.waiting.get(project).question };
  if (index.judging.has(project)) return { kind: 'deciding', text: '' };
  if (index.last.has(project)) return { kind: 'handled', text: index.last.get(project) };
  return { kind: 'none', text: '' };
}

export function projectSays(view, project) {
  return saysFrom(saysIndex(view), project);
}

export function projectRows(agents, view) {
  const index = saysIndex(view);
  const handled = new Map(view.handledByProject.map((entry) => [entry.path, entry.count]));
  const overrides = new Map(view.projects.map((entry) => [entry.path, entry.mode]));
  const rows = groupByProject(agents).map((group) => ({
    project: group.project,
    color: group.color,
    names: group.agents.map((agent) => agent.name),
    counts: fleetCounts(group.agents),
    needs: index.waiting.has(group.project) ? index.waiting.get(group.project).count : 0,
    says: saysFrom(index, group.project),
    handled: handled.get(group.project) || 0,
    mode: overrides.get(group.project) || view.mode,
    override: overrides.get(group.project) || null,
  }));
  return rows.sort((a, b) => Number(b.needs > 0) - Number(a.needs > 0) || b.counts.total - a.counts.total);
}

export function filterProjects(rows, query, filter) {
  const wanted = String(query || '').trim().toLowerCase();
  return rows.filter((row) => {
    if (filter === 'needs' && !row.needs) return false;
    if (filter === 'bypass' && row.mode !== 'bypass') return false;
    return !wanted || row.project.toLowerCase().includes(wanted) || row.names.some((name) => name.toLowerCase().includes(wanted));
  });
}

export function projectFilterCounts(rows) {
  return { all: rows.length, needs: rows.filter((row) => row.needs > 0).length, bypass: rows.filter((row) => row.mode === 'bypass').length };
}

export function decisionProjects(view) {
  return [...new Set([...view.decisions, ...view.handled].map((item) => item.project).filter(Boolean))].sort();
}

export function shownDecisions(view, filter, project, now) {
  const inProject = (item) => !project || item.project === project;
  const overview = filter !== 'deciding' && filter !== 'handled';
  return {
    needs: overview ? needsYou(view).filter(inProject) : [],
    deciding: overview || filter === 'deciding' ? deciding(view).filter(inProject) : [],
    handled: view.handled.filter(inProject).filter((entry) => (overview ? now - entry.at < RECENT_MS : filter === 'handled')),
  };
}

export function pickDecision(selected, shown) {
  const pending = [...shown.needs, ...shown.deciding];
  if (pending.some((decision) => decision.id === selected)) return selected;
  return shown.needs.length ? shown.needs[0].id : null;
}

export function decisionMeta(decision, agent) {
  const model = agent ? shortModel(agent.model) : '';
  return [decision.project, [label(decision.kind), model].filter(Boolean).join(' '), agent && agent.branch ? agent.branch : null, decision.tag].filter(Boolean);
}

export function suggestionText(suggestion) {
  if (!suggestion) return '';
  return suggestion.text || suggestion.summary || suggestion.keys.join(' ');
}

export function shortAge(ms) {
  return duration(ms).match(/^\d+\D/)[0];
}

export function outcomeMark(entry) {
  return OUTCOME_MARKS[entry.outcome] || UNKNOWN_MARK;
}

export function byWord(entry) {
  return BY_WORDS[entry.by] || entry.by;
}

export function mergeActivity(newer, older) {
  const bySeq = new Map();
  for (const entry of [...newer, ...older]) if (entry && Number.isFinite(entry.seq) && !bySeq.has(entry.seq)) bySeq.set(entry.seq, entry);
  return [...bySeq.values()].sort((a, b) => b.seq - a.seq);
}

export function activityPage(message) {
  return { items: listOf(message && message.items).map(activityOf).filter(Boolean), next: message && Number.isFinite(message.next) ? message.next : null };
}

export function instructionTabs(agents, view) {
  return [null, ...new Set([...agents.map((agent) => agent.project), ...view.projects.map((entry) => entry.path)].filter(Boolean))];
}

export function instructionsFile(message) {
  return { project: textOf(message.project), text: typeof message.text === 'string' ? message.text : '', path: textOf(message.path) || '' };
}

export function instructionsTooLong(text) {
  return new TextEncoder().encode(text).length > INSTRUCTIONS_MAX_BYTES;
}

export function canSaveInstructions(saved, text) {
  return saved !== null && text !== saved && !instructionsTooLong(text);
}

export function needsBypassConfirm(mode, current) {
  return mode === 'bypass' && current !== 'bypass';
}

export function viewRequest() {
  return { type: 'orchestrator' };
}

export function modeMessage(mode) {
  return { type: 'orchestratorSet', mode };
}

export function projectModeMessage(path, mode) {
  return { type: 'orchestratorSet', project: { path, mode: MODES.includes(mode) ? mode : null } };
}

export function enabledMessage(enabled) {
  return { type: 'orchestratorSet', enabled };
}

export function turnOnMessage(primary, mode) {
  return { type: 'orchestratorSet', enabled: true, primary, mode };
}

export function engineMessage(slot, kind) {
  return { type: 'orchestratorSet', [slot]: kind || null };
}

export function modelMessage(slot, model) {
  return { type: 'orchestratorSet', [`${slot}Model`]: String(model || '').trim() || null };
}

export function answerMessage(decision, how, options = {}) {
  const message = { type: 'orchestratorAnswer', decision, how };
  if (how === 'text') message.text = options.text;
  if (how === 'keys') message.keys = options.keys;
  const rule = String(options.remember || '').trim();
  if (rule && how !== 'dismiss') message.remember = rule;
  return message;
}

export function suggestionAnswer(decision, edited, remember) {
  const typed = typeof edited === 'string' ? edited.trim() : '';
  const offered = decision.suggestion && decision.suggestion.text ? decision.suggestion.text.trim() : '';
  if (typed && typed !== offered) return answerMessage(decision.id, 'text', { text: typed, remember });
  return decision.suggestion ? answerMessage(decision.id, 'suggestion', { remember }) : null;
}

export function chatMessage(view, text) {
  const typed = String(text || '').trim();
  return view.enabled && !view.thinking && typed ? { type: 'orchestratorChat', text: typed } : null;
}

export function clearChatMessage() {
  return { type: 'orchestratorClearChat' };
}

export function instructionsRequest(project) {
  return { type: 'orchestratorInstructions', project: project || null };
}

export function saveInstructionsMessage(project, text) {
  return { type: 'orchestratorSaveInstructions', project: project || null, text };
}

export function activityRequest(before) {
  return { type: 'orchestratorActivity', before: Number.isFinite(before) ? before : null };
}

export function decisionSignature(decision) {
  return JSON.stringify([decision.id, decision.status, decision.name, decision.kind, decision.project, decision.since, decision.question, decision.tag, decision.problem, decision.suggestion]);
}

export function detailSignature(decision, agent) {
  return JSON.stringify([decision, decisionMeta(decision, agent)]);
}

export function activitySignature(entry) {
  return JSON.stringify([entry.seq, entry.at, entry.name, entry.project, entry.summary, entry.by, entry.outcome]);
}

export function chatSignature(entry) {
  return JSON.stringify([entry.role, entry.text, entry.actions, entry.failed]);
}

export function chatKeys(chat) {
  const seen = new Map();
  return chat.map((entry) => {
    const base = `${entry.role}:${entry.at}`;
    const count = seen.get(base) || 0;
    seen.set(base, count + 1);
    return count ? `${base}#${count}` : base;
  });
}
