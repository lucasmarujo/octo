import type { Ask, TextTone } from './agents';
import { title } from './agents';
import { atLeast } from './chat';
import type { Column, Mood } from './model';
import model from './model';
import type {
  ActivityBy,
  ActivityEntry,
  ActivityOutcome,
  AgentKind,
  AgentView,
  AnswerHow,
  AskedTool,
  Decision,
  Engine,
  HandledCount,
  Mode,
  OctoChatEntry,
  OrchestratorAnswerMsg,
  OrchestratorAskActivityMsg,
  OrchestratorAskInstructionsMsg,
  OrchestratorChatMsg,
  OrchestratorSaveInstructionsMsg,
  OrchestratorSetMsg,
  OrchestratorView,
  ProjectMode,
  Suggestion,
} from './protocol';

export type OctoTab = 'chat' | 'decisions' | 'activity' | 'projects';
export type EngineSlot = 'primary' | 'secondary';
export type SaysKind = 'needs' | 'deciding' | 'handled' | 'none';
export type OctoGate = 'offline' | 'intro' | 'stale' | 'ready';

export type FleetCounts = Record<Column, number> & {
  total: number;
};

export type Says = {
  kind: SaysKind;
  text: string;
};

export type ProjectRow = {
  project: string;
  color: string;
  names: string[];
  counts: FleetCounts;
  needs: number;
  says: Says;
  handled: number;
  mode: Mode;
  override: Mode | null;
};

export type Segment = {
  column: Column;
  share: number;
};

export type OutcomeMark = {
  text: string;
  tone: TextTone;
};

export type DecisionItem =
  | { type: 'head'; key: string; text: string; count: number }
  | { type: 'needs'; key: string; decision: Decision }
  | { type: 'deciding'; key: string; decision: Decision }
  | { type: 'handled'; key: string; entry: ActivityEntry }
  | { type: 'empty'; key: string };

export type ChatRow = {
  key: string;
  entry: OctoChatEntry;
};

export type ActivityPage = {
  items: ActivityEntry[];
  next: number | null;
};

export type InstructionsFile = {
  project: string | null;
  text: string;
  path: string;
};

export type AnswerOptions = {
  text?: string;
  keys?: string[];
  remember?: string;
};

export type EntryLine = {
  text: string;
  needs: number;
};

type Unsure<T> = { [K in keyof T]?: unknown } | null | undefined;

type SaysIndex = {
  waiting: Map<string, { question: string; count: number }>;
  judging: Set<string>;
  last: Map<string, string>;
};

export const OCTO_TABS: OctoTab[] = ['chat', 'decisions', 'activity', 'projects'];
export const TAB_NAMES: Record<OctoTab, string> = { chat: 'Chat', decisions: 'Decisions', activity: 'Activity', projects: 'Projects' };
export const MODES: Mode[] = ['observe', 'moderate', 'bypass'];
export const DEFAULT_MODE: Mode = 'moderate';
const BYPASS: Mode = 'bypass';
const ENGINE_KINDS = ['claude', 'codex'];
export const RECENT_MS = 600_000;
export const CHAT_CARDS = 3;
export const ACTIVITY_MAX = 1000;
export const INSTRUCTIONS_MAX_BYTES = 65_536;
export const GLOBAL_FILE = 'OCTO.md';
const OCTO_SINCE = [0, 2, 39];
const COLUMNS: Column[] = ['working', 'needs', 'done', 'idle'];
const BYS: ActivityBy[] = ['octo', 'rule', 'you'];
const OUTCOMES: ActivityOutcome[] = ['answered', 'denied', 'escalated', 'failed'];
const UNKNOWN_BY: ActivityBy = 'octo';
const UNKNOWN_OUTCOME: ActivityOutcome = 'failed';

export const MODE_NAMES: Record<Mode, string> = { observe: 'Observe', moderate: 'Moderate', bypass: 'Bypass' };
export const MODE_TEXTS: Record<Mode, string> = {
  observe: 'Octo watches and reports. It never answers an agent. Every question comes to you, with what it would have said.',
  moderate: 'Octo answers what your rules and the task already settle. Anything truly open, irreversible or outside the task waits for you.',
  bypass: 'Octo decides everything and allows everything, with no limits. Nothing waits for you. You read what it did in Activity.',
};
const BY_WORDS: Record<ActivityBy, string> = { octo: 'judgment', rule: 'rule', you: 'you' };
const OUTCOME_MARKS: Record<ActivityOutcome, OutcomeMark> = {
  answered: { text: '✓', tone: 'green' },
  denied: { text: '✕', tone: 'red' },
  escalated: { text: '‖', tone: 'yellow' },
  failed: { text: '!', tone: 'red' },
};
const BYPASS_TEXT = 'Octo will answer every question and allow everything on its own, with no block list and no limits. Nothing waits for you.';

export function octoCapable(version: string | null): boolean {
  return atLeast(version, OCTO_SINCE);
}

function listOf(value: unknown): unknown[] {
  return Array.isArray(value) ? value : [];
}

function textOf(value: unknown): string | null {
  return typeof value === 'string' && value ? value : null;
}

function countOf(value: unknown): number {
  return typeof value === 'number' && Number.isFinite(value) && value > 0 ? value : 0;
}

function modeOf(value: unknown): Mode | null {
  return MODES.find(mode => mode === value) || null;
}

function strings(value: unknown): string[] {
  return listOf(value).filter((entry): entry is string => typeof entry === 'string');
}

function present<T>(value: T | null): value is T {
  return value !== null;
}

function suggestionOf(value: unknown): Suggestion | null {
  const entry = value as Unsure<Suggestion>;
  if (!entry) return null;
  const keys = strings(entry.keys).filter(Boolean);
  const text = textOf(entry.text);
  if (!keys.length && !text) return null;
  return { keys: text ? null : keys, text, summary: textOf(entry.summary) || '', reason: textOf(entry.reason) || '' };
}

function toolOf(value: unknown): AskedTool | null {
  const entry = value as Unsure<AskedTool>;
  const name = entry ? textOf(entry.name) : null;
  return entry && name ? { name, detail: textOf(entry.detail) || '' } : null;
}

function decisionOf(value: unknown): Decision | null {
  const entry = value as Unsure<Decision>;
  if (!entry || typeof entry.id !== 'number' || !Number.isFinite(entry.id)) return null;
  return {
    id: entry.id,
    agent: countOf(entry.agent),
    name: textOf(entry.name) || '',
    kind: textOf(entry.kind) || '',
    session: textOf(entry.session) || '',
    project: textOf(entry.project) || '',
    since: countOf(entry.since),
    status: entry.status === 'needsYou' ? 'needsYou' : 'deciding',
    question: textOf(entry.question) || '',
    context: strings(entry.context),
    tool: toolOf(entry.tool),
    suggestion: suggestionOf(entry.suggestion),
    tag: textOf(entry.tag),
    problem: textOf(entry.problem),
  };
}

function activityOf(value: unknown): ActivityEntry | null {
  const entry = value as Unsure<ActivityEntry>;
  if (!entry || typeof entry.seq !== 'number' || !Number.isFinite(entry.seq)) return null;
  return {
    seq: entry.seq,
    at: countOf(entry.at),
    agent: countOf(entry.agent),
    name: textOf(entry.name) || '',
    kind: textOf(entry.kind) || '',
    project: textOf(entry.project) || '',
    question: textOf(entry.question) || '',
    summary: textOf(entry.summary) || '',
    by: BYS.find(by => by === entry.by) || UNKNOWN_BY,
    outcome: OUTCOMES.find(outcome => outcome === entry.outcome) || UNKNOWN_OUTCOME,
    mode: modeOf(entry.mode) || DEFAULT_MODE,
  };
}

function chatOf(value: unknown): OctoChatEntry | null {
  const entry = value as Unsure<OctoChatEntry>;
  if (!entry || typeof entry.text !== 'string') return null;
  return { at: countOf(entry.at), role: entry.role === 'you' ? 'you' : 'octo', text: entry.text, actions: strings(entry.actions).filter(Boolean), failed: entry.failed === true };
}

function projectModeOf(value: unknown): ProjectMode | null {
  const entry = value as Unsure<ProjectMode>;
  const mode = entry ? modeOf(entry.mode) : null;
  return entry && typeof entry.path === 'string' && mode ? { path: entry.path, mode } : null;
}

function engineOf(value: unknown): Engine | null {
  const entry = value as Unsure<Engine>;
  const kind = entry ? textOf(entry.kind) : null;
  return entry && kind ? { kind, installed: entry.installed === true } : null;
}

function handledCountOf(value: unknown): HandledCount | null {
  const entry = value as Unsure<HandledCount>;
  return entry && typeof entry.path === 'string' ? { path: entry.path, count: countOf(entry.count) } : null;
}

export function readView(message: unknown): OrchestratorView {
  const view = (message && typeof message === 'object' ? message : {}) as { [K in keyof OrchestratorView]?: unknown };
  return {
    online: view.online !== false,
    enabled: view.enabled === true,
    mode: modeOf(view.mode) || DEFAULT_MODE,
    projects: listOf(view.projects).map(projectModeOf).filter(present),
    primary: textOf(view.primary),
    secondary: textOf(view.secondary),
    primaryModel: textOf(view.primaryModel),
    secondaryModel: textOf(view.secondaryModel),
    engines: listOf(view.engines).map(engineOf).filter(present),
    decisions: listOf(view.decisions).map(decisionOf).filter(present),
    handled: listOf(view.handled).map(activityOf).filter(present),
    handledToday: countOf(view.handledToday),
    handledByProject: listOf(view.handledByProject).map(handledCountOf).filter(present),
    chat: listOf(view.chat).map(chatOf).filter(present),
    thinking: view.thinking === true,
    problem: textOf(view.problem),
  };
}

function same(first: unknown, second: unknown): boolean {
  if (first === second) return true;
  if (typeof first !== 'object' || typeof second !== 'object' || first === null || second === null) return false;
  if (Array.isArray(first) !== Array.isArray(second)) return false;
  const left = first as Record<string, unknown>;
  const right = second as Record<string, unknown>;
  const names = Object.keys(left);
  return names.length === Object.keys(right).length && names.every(name => same(left[name], right[name]));
}

function kept<T>(before: T[], after: T[], keyOf: (item: T, index: number) => string | number): T[] {
  const known = new Map(before.map((item, index) => [keyOf(item, index), item]));
  let unchanged = before.length === after.length;
  const list = after.map((item, index) => {
    const old = known.get(keyOf(item, index));
    const held = old !== undefined && same(old, item) ? old : item;
    if (held !== before[index]) unchanged = false;
    return held;
  });
  return unchanged ? before : list;
}

export function settled(previous: OrchestratorView | null, next: OrchestratorView): OrchestratorView {
  if (!previous) return next;
  const view: OrchestratorView = {
    ...next,
    projects: kept(previous.projects, next.projects, entry => entry.path),
    engines: kept(previous.engines, next.engines, entry => entry.kind),
    decisions: kept(previous.decisions, next.decisions, entry => entry.id),
    handled: kept(previous.handled, next.handled, entry => entry.seq),
    handledByProject: kept(previous.handledByProject, next.handledByProject, entry => entry.path),
    chat: kept(previous.chat, next.chat, (_entry, index) => index),
  };
  const names = Object.keys(view) as (keyof OrchestratorView)[];
  return names.every(name => previous[name] === view[name]) ? previous : view;
}

export function needsYou(view: OrchestratorView): Decision[] {
  return view.decisions.filter(decision => decision.status === 'needsYou');
}

export function deciding(view: OrchestratorView): Decision[] {
  return view.decisions.filter(decision => decision.status === 'deciding');
}

export function needsCount(view: OrchestratorView | null): number {
  return view && view.enabled && view.online ? needsYou(view).length : 0;
}

export function gateOf(view: OrchestratorView): OctoGate {
  if (view.online) return view.enabled ? 'ready' : 'intro';
  return view.enabled ? 'stale' : 'offline';
}

function plural(count: number, one: string, many: string): string {
  return `${count} ${count === 1 ? one : many}`;
}

export function entryLine(view: OrchestratorView | null): EntryLine {
  if (!view) return { text: 'Asking the computer…', needs: 0 };
  if (!view.online) return { text: 'Open Octo on the computer', needs: 0 };
  if (!view.enabled) return { text: 'Off · let it answer your agents', needs: 0 };
  const needs = needsCount(view);
  const handled = `${view.handledToday} handled today`;
  return { text: `${MODE_NAMES[view.mode].toLowerCase()} · ${needs ? `${plural(needs, 'needs', 'need')} you` : handled}`, needs };
}

export function headLine(view: OrchestratorView | null): string {
  if (!view || !view.online || !view.enabled) return entryLine(view).text;
  const needs = needsCount(view);
  return needs ? `${plural(needs, 'needs', 'need')} you · ${view.handledToday} handled today` : `${view.handledToday} handled today`;
}

export function fleetCounts(agents: AgentView[]): FleetCounts {
  const counts: FleetCounts = { total: agents.length, working: 0, needs: 0, done: 0, idle: 0 };
  for (const agent of agents) counts[model.column(agent)] += 1;
  return counts;
}

export function segments(counts: FleetCounts): Segment[] {
  if (!counts.total) return [];
  return COLUMNS.filter(column => counts[column] > 0).map(column => ({ column, share: counts[column] / counts.total }));
}

function saysIndex(view: OrchestratorView): SaysIndex {
  const index: SaysIndex = { waiting: new Map(), judging: new Set(), last: new Map() };
  for (const decision of view.decisions) {
    const waiting = index.waiting.get(decision.project);
    if (decision.status !== 'needsYou') index.judging.add(decision.project);
    else if (waiting) waiting.count += 1;
    else index.waiting.set(decision.project, { question: decision.question, count: 1 });
  }
  for (const entry of view.handled) if (!index.last.has(entry.project)) index.last.set(entry.project, entry.summary);
  return index;
}

function saysFrom(index: SaysIndex, project: string): Says {
  const waiting = index.waiting.get(project);
  if (waiting) return { kind: 'needs', text: waiting.question };
  if (index.judging.has(project)) return { kind: 'deciding', text: '' };
  const last = index.last.get(project);
  return last === undefined ? { kind: 'none', text: '' } : { kind: 'handled', text: last };
}

export function saysText(says: Says): string {
  if (says.kind === 'needs') return `Waiting on you: ${says.text}`;
  if (says.kind === 'deciding') return 'Deciding…';
  return says.kind === 'handled' ? says.text : 'Nothing asked yet';
}

export function projectRows(agents: AgentView[], view: OrchestratorView): ProjectRow[] {
  const index = saysIndex(view);
  const handled = new Map(view.handledByProject.map(entry => [entry.path, entry.count]));
  const overrides = new Map(view.projects.map(entry => [entry.path, entry.mode]));
  const groups = new Map<string, AgentView[]>();
  for (const agent of agents) {
    const members = groups.get(agent.project);
    if (members) members.push(agent);
    else groups.set(agent.project, [agent]);
  }
  const rows = [...groups].map(([project, members]): ProjectRow => {
    const waiting = index.waiting.get(project);
    const override = overrides.get(project) || null;
    return {
      project,
      color: members[0].color,
      names: members.map(agent => agent.name),
      counts: fleetCounts(members),
      needs: waiting ? waiting.count : 0,
      says: saysFrom(index, project),
      handled: handled.get(project) || 0,
      mode: override || view.mode,
      override,
    };
  });
  return rows.sort((first, second) => Number(second.needs > 0) - Number(first.needs > 0) || second.counts.total - first.counts.total);
}

export function sameRow(first: ProjectRow, second: ProjectRow): boolean {
  return same(first, second);
}

export function octoMood(view: OrchestratorView | null): Mood {
  if (!view || !view.online || !view.enabled) return 'idle';
  if (needsCount(view)) return 'needs';
  return view.thinking || deciding(view).length ? 'working' : 'idle';
}

export function engineKinds(view: OrchestratorView): string[] {
  return view.engines.length ? view.engines.map(engine => engine.kind) : ENGINE_KINDS;
}

export function installedEngines(view: OrchestratorView): string[] {
  return view.engines.filter(engine => engine.installed).map(engine => engine.kind);
}

export function startChoice(view: OrchestratorView, picked: string | null): string | null {
  const installed = installedEngines(view);
  if (picked && installed.includes(picked)) return picked;
  if (view.primary && installed.includes(view.primary)) return view.primary;
  return installed[0] || null;
}

export function secondaryChoices(view: OrchestratorView): string[] {
  return installedEngines(view).filter(kind => kind !== view.primary);
}

export function engineName(kind: string): string {
  return title(kind as AgentKind);
}

export function suggestionText(suggestion: Suggestion | null): string {
  if (!suggestion) return '';
  return suggestion.text || suggestion.summary || (suggestion.keys || []).join(' ');
}

export function decisionMeta(decision: Decision): string {
  return [decision.project, decision.kind].filter(Boolean).join(' · ');
}

export function decisionItems(view: OrchestratorView, now: number): DecisionItem[] {
  const needs = needsYou(view);
  const judging = deciding(view);
  const recent = view.handled.filter(entry => now - entry.at < RECENT_MS);
  const items: DecisionItem[] = [];
  if (needs.length) items.push({ type: 'head', key: 'head:needs', text: 'Needs you', count: needs.length });
  else items.push({ type: 'empty', key: 'empty' });
  for (const decision of needs) items.push({ type: 'needs', key: `needs:${decision.id}`, decision });
  if (judging.length) items.push({ type: 'head', key: 'head:deciding', text: 'Octo is deciding', count: judging.length });
  for (const decision of judging) items.push({ type: 'deciding', key: `deciding:${decision.id}`, decision });
  if (recent.length) items.push({ type: 'head', key: 'head:handled', text: 'Handled in the last 10 minutes', count: recent.length });
  for (const entry of recent) items.push({ type: 'handled', key: `handled:${entry.seq}`, entry });
  return items;
}

export function chatCards(view: OrchestratorView): Decision[] {
  return needsYou(view).slice(0, CHAT_CARDS);
}

export function moreCards(view: OrchestratorView): number {
  return Math.max(0, needsYou(view).length - CHAT_CARDS);
}

export function chatRows(chat: OctoChatEntry[]): ChatRow[] {
  const seen = new Map<string, number>();
  return chat.map(entry => {
    const base = `${entry.role}:${entry.at}`;
    const count = seen.get(base) || 0;
    seen.set(base, count + 1);
    return { key: count ? `${base}#${count}` : base, entry };
  });
}

export function shortAge(ms: number): string {
  const found = /^\d+\D/.exec(model.duration(Math.max(0, ms)));
  return found ? found[0] : '';
}

export function outcomeMark(entry: ActivityEntry): OutcomeMark {
  return OUTCOME_MARKS[entry.outcome];
}

export function byWord(entry: ActivityEntry): string {
  return BY_WORDS[entry.by];
}

export function activityNote(entry: ActivityEntry): string {
  return [entry.project, MODE_NAMES[entry.mode].toLowerCase(), byWord(entry)].filter(Boolean).join(' · ');
}

export function mergeActivity(newer: ActivityEntry[], older: ActivityEntry[]): ActivityEntry[] {
  const bySeq = new Map<number, ActivityEntry>();
  for (const entry of [...newer, ...older]) if (!bySeq.has(entry.seq)) bySeq.set(entry.seq, entry);
  return [...bySeq.values()].sort((first, second) => second.seq - first.seq);
}

export function activityPage(message: unknown): ActivityPage {
  const page = message as Unsure<ActivityPage>;
  const next = page ? page.next : null;
  return { items: listOf(page ? page.items : null).map(activityOf).filter(present), next: typeof next === 'number' && Number.isFinite(next) ? next : null };
}

export function instructionsFile(message: unknown): InstructionsFile {
  const file = message as Unsure<InstructionsFile>;
  return { project: file ? textOf(file.project) : null, text: file && typeof file.text === 'string' ? file.text : '', path: (file && textOf(file.path)) || '' };
}

export function utf8Length(text: string): number {
  let bytes = 0;
  for (const symbol of text) {
    const code = symbol.codePointAt(0) || 0;
    bytes += code < 0x80 ? 1 : code < 0x800 ? 2 : code < 0x10000 ? 3 : 4;
  }
  return bytes;
}

export function instructionsTooLong(text: string): boolean {
  return utf8Length(text) > INSTRUCTIONS_MAX_BYTES;
}

export function canSaveInstructions(saved: string | null, text: string): boolean {
  return saved !== null && text !== saved && !instructionsTooLong(text);
}

export function needsBypassConfirm(mode: Mode | null, current: Mode): boolean {
  return mode === BYPASS && current !== BYPASS;
}

export function projectBypassConfirm(view: OrchestratorView, override: Mode | null, mode: Mode | null): boolean {
  return needsBypassConfirm(mode || view.mode, override || view.mode);
}

export function enableBypassConfirm(view: OrchestratorView): boolean {
  return !view.enabled && view.mode === BYPASS;
}

export function bypassAsk(project: string | null): Ask {
  return { title: project ? `Run ${project} in Bypass?` : 'Switch Octo to Bypass?', text: BYPASS_TEXT, action: 'Use Bypass' };
}

export function viewRequest(): { type: 'orchestrator' } {
  return { type: 'orchestrator' };
}

export function modeMessage(mode: Mode): OrchestratorSetMsg {
  return { type: 'orchestratorSet', mode };
}

export function projectModeMessage(path: string, mode: Mode | null): OrchestratorSetMsg {
  return { type: 'orchestratorSet', project: { path, mode } };
}

export function enabledMessage(enabled: boolean): OrchestratorSetMsg {
  return { type: 'orchestratorSet', enabled };
}

export function turnOnMessage(primary: string, mode: Mode): OrchestratorSetMsg {
  return { type: 'orchestratorSet', enabled: true, primary, mode };
}

export function engineMessage(slot: EngineSlot, kind: string | null): OrchestratorSetMsg {
  return slot === 'primary' ? { type: 'orchestratorSet', primary: kind || null } : { type: 'orchestratorSet', secondary: kind || null };
}

export function answerMessage(decision: number, how: AnswerHow, options: AnswerOptions = {}): OrchestratorAnswerMsg {
  const message: OrchestratorAnswerMsg = { type: 'orchestratorAnswer', decision, how };
  if (how === 'text') message.text = options.text || '';
  if (how === 'keys') message.keys = options.keys || [];
  const rule = (options.remember || '').trim();
  if (rule && how !== 'dismiss') message.remember = rule;
  return message;
}

export function suggestionAnswer(decision: Decision, edited: string | null, remember: string): OrchestratorAnswerMsg | null {
  const typed = (edited || '').trim();
  if (typed && typed !== suggestionText(decision.suggestion).trim()) return answerMessage(decision.id, 'text', { text: typed, remember });
  return decision.suggestion ? answerMessage(decision.id, 'suggestion', { remember }) : null;
}

export function chatMessage(view: OrchestratorView | null, text: string): OrchestratorChatMsg | null {
  const typed = text.trim();
  return view && view.online && view.enabled && !view.thinking && typed ? { type: 'orchestratorChat', text: typed } : null;
}

export function clearChatMessage(): { type: 'orchestratorClearChat' } {
  return { type: 'orchestratorClearChat' };
}

export function instructionsRequest(project: string | null): OrchestratorAskInstructionsMsg {
  return { type: 'orchestratorInstructions', project: project || null };
}

export function saveInstructionsMessage(project: string | null, text: string): OrchestratorSaveInstructionsMsg {
  return { type: 'orchestratorSaveInstructions', project: project || null, text };
}

export function activityRequest(before: number | null): OrchestratorAskActivityMsg {
  return { type: 'orchestratorActivity', before: typeof before === 'number' && Number.isFinite(before) ? before : null };
}
