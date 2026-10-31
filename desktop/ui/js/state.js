import { storage } from './dom.js';

const SESSION_KEY = 'orkai.session';
const EXPANDED_KEY = 'orkai.expanded';
const WHO_KEY = 'orkai.who';
const WHERE_KEY = 'orkai.where';
const WORKTREE_KEY = 'orkai.worktree';
const BYPASS_KEY = 'orkai.bypass';
const TABS_KEY = 'orkai.tabs';
const VIEW_KEY = 'orkai.view';
const PROJECT_KEY = 'orkai.project';

export const VIEWS = ['board', 'list', 'grid'];

export const state = {
  host: '',
  version: '',
  overview: null,
  usage: null,
  usageAt: 0,
  skew: 0,
  frame: 0,
  session: storage.get(SESSION_KEY),
  pendingSession: null,
  expanded: new Set(storage.json(EXPANDED_KEY, [])),
  who: storage.get(WHO_KEY),
  where: storage.json(WHERE_KEY, {}),
  worktree: storage.json(WORKTREE_KEY, null),
  bypass: storage.get(BYPASS_KEY) === 'on',
  history: [],
  recall: -1,
  folders: null,
  tabs: storage.json(TABS_KEY, []),
  active: null,
  view: VIEWS.includes(storage.get(VIEW_KEY)) ? storage.get(VIEW_KEY) : 'board',
  projects: storage.json(PROJECT_KEY, {}),
  previews: new Map(),
  peekAt: 0,
};

export function findAgent(id) {
  return agents().find((agent) => agent.id === id) || null;
}

export function saveTabs() {
  storage.set(TABS_KEY, JSON.stringify(state.tabs));
}

export const now = () => Date.now() - state.skew;

export function agents() {
  return state.overview ? state.overview.agents : [];
}

export function inSession(name = state.session) {
  return agents().filter((agent) => agent.session === name);
}

export function projectsInSession() {
  return [...new Set(inSession().map((agent) => agent.project))];
}

export function currentProject() {
  const chosen = state.projects[state.session];
  return chosen && projectsInSession().includes(chosen) ? chosen : null;
}

export function setProject(project) {
  if (project) state.projects[state.session] = project;
  else delete state.projects[state.session];
  storage.set(PROJECT_KEY, JSON.stringify(state.projects));
}

export function visible() {
  const project = currentProject();
  return project ? inSession().filter((agent) => agent.project === project) : inSession();
}

export function setView(view) {
  state.view = view;
  storage.set(VIEW_KEY, view);
}

export function selectSession(name) {
  state.session = name;
  storage.set(SESSION_KEY, name);
}

export function toggleExpanded(name) {
  if (state.expanded.has(name)) state.expanded.delete(name);
  else state.expanded.add(name);
  storage.set(EXPANDED_KEY, JSON.stringify([...state.expanded]));
}

export function installedKinds() {
  return state.overview ? state.overview.kinds : [];
}

export function currentWho() {
  const kinds = installedKinds();
  if (state.who && kinds.includes(state.who)) return state.who;
  return kinds.includes('claude') ? 'claude' : kinds[0] || 'terminal';
}

export function setWho(kind) {
  state.who = kind;
  storage.set(WHO_KEY, kind);
}

export function currentWhere() {
  const chosen = state.where[state.session];
  if (chosen) return chosen;
  const overview = state.overview;
  if (!overview) return '~';
  return overview.dirs[state.session] || overview.whereDir || '~';
}

export function setWhere(path) {
  state.where[state.session] = path;
  storage.set(WHERE_KEY, JSON.stringify(state.where));
}

export function worktreeOn() {
  if (state.worktree !== null) return state.worktree;
  return Boolean(state.overview && state.overview.worktree);
}

export function setWorktree(on) {
  state.worktree = on;
  storage.set(WORKTREE_KEY, JSON.stringify(on));
}

export function setBypass(on) {
  state.bypass = on;
  storage.set(BYPASS_KEY, on ? 'on' : null);
}
