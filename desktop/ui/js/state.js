import { storage } from './dom.js';
import { DEFAULT_GRID_SIZE, GRID_SIZES } from './format.js';

const SESSION_KEY = 'orkai.session';
const EXPANDED_KEY = 'orkai.expanded';
const WHO_KEY = 'orkai.who';
const WHERE_KEY = 'orkai.where';
const WORKTREE_KEY = 'orkai.worktree';
const BYPASS_KEY = 'orkai.bypass';
const REMOTE_KEY = 'orkai.remote';
const TABS_KEY = 'orkai.tabs';
const VIEW_KEY = 'orkai.view';
const GRID_KEY = 'orkai.grid';
const PROJECT_KEY = 'orkai.project';
const USAGE_KEY = 'orkai.usage';
const WORKSPACES_KEY = 'octo.workspaces';
const savedWorkspaces = storage.json(WORKSPACES_KEY, []);

export const VIEWS = ['board', 'list', 'grid'];

export const state = {
  host: '',
  version: '',
  overview: null,
  usage: null,
  usageAt: 0,
  usageOpen: storage.get(USAGE_KEY) === 'open',
  skew: 0,
  frame: 0,
  session: storage.get(SESSION_KEY),
  pendingSession: null,
  pendingWorkspace: null,
  pendingRename: null,
  workspaces: Array.isArray(savedWorkspaces) ? savedWorkspaces.filter((item) => item && typeof item.id === 'string' && typeof item.name === 'string' && Array.isArray(item.sessions) && item.sessions.every((name) => typeof name === 'string')) : [],
  uncategorizedCollapsed: storage.get('octo.uncategorizedCollapsed') === 'on',
  expanded: new Set(storage.json(EXPANDED_KEY, [])),
  who: storage.get(WHO_KEY),
  where: storage.json(WHERE_KEY, {}),
  worktree: storage.json(WORKTREE_KEY, null),
  bypass: storage.get(BYPASS_KEY) === 'on',
  remote: storage.get(REMOTE_KEY) === 'on',
  history: [],
  recall: -1,
  folders: null,
  tabs: storage.json(TABS_KEY, []),
  active: null,
  view: VIEWS.includes(storage.get(VIEW_KEY)) ? storage.get(VIEW_KEY) : 'board',
  gridSize: GRID_SIZES.includes(storage.get(GRID_KEY)) ? storage.get(GRID_KEY) : DEFAULT_GRID_SIZE,
  projects: storage.json(PROJECT_KEY, {}),
  previews: new Map(),
  peekAt: 0,
  details: null,
  detailsAt: 0,
  home: null,
  selected: null,
  skills: null,
  updateReady: null,
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

const saveWorkspaces = () => storage.set(WORKSPACES_KEY, JSON.stringify(state.workspaces));

export function workspaceFor(session = state.session) {
  return state.workspaces.find((item) => item.sessions.includes(session)) || null;
}

export function createWorkspace(name) {
  name = name.trim();
  if (!name || state.workspaces.some((item) => item.name === name)) return null;
  const workspace = { id: crypto.randomUUID(), name, sessions: [], collapsed: false };
  state.workspaces.push(workspace);
  saveWorkspaces();
  return workspace;
}

export function renameWorkspace(id, name) {
  name = name.trim();
  const workspace = state.workspaces.find((item) => item.id === id);
  if (!workspace || !name || state.workspaces.some((item) => item.id !== id && item.name === name)) return false;
  workspace.name = name;
  saveWorkspaces();
  return true;
}

export function assignWorkspace(session, id, before = null) {
  const workspace = state.workspaces.find((item) => item.id === id);
  if (id && !workspace) return;
  for (const item of state.workspaces) item.sessions = item.sessions.filter((name) => name !== session);
  if (workspace) {
    const at = workspace.sessions.indexOf(before);
    workspace.sessions.splice(at < 0 ? workspace.sessions.length : at, 0, session);
  }
  saveWorkspaces();
}

export function moveWorkspace(id, before = null) {
  const workspace = state.workspaces.find((item) => item.id === id);
  if (!workspace || id === before) return;
  state.workspaces = state.workspaces.filter((item) => item !== workspace);
  const at = state.workspaces.findIndex((item) => item.id === before);
  state.workspaces.splice(at < 0 ? state.workspaces.length : at, 0, workspace);
  saveWorkspaces();
}

export function toggleWorkspace(id) {
  if (id === 'uncategorized') {
    state.uncategorizedCollapsed = !state.uncategorizedCollapsed;
    storage.set('octo.uncategorizedCollapsed', state.uncategorizedCollapsed ? 'on' : null);
    return;
  }
  const workspace = state.workspaces.find((item) => item.id === id);
  if (!workspace) return;
  workspace.collapsed = !workspace.collapsed;
  saveWorkspaces();
}

export function removeWorkspace(id) {
  state.workspaces = state.workspaces.filter((item) => item.id !== id);
  saveWorkspaces();
}

export function reconcileWorkspaces(names) {
  let changed = false;
  if (state.pendingRename && names.includes(state.pendingRename.to) && !names.includes(state.pendingRename.from)) {
    for (const item of state.workspaces) item.sessions = item.sessions.map((name) => name === state.pendingRename.from ? state.pendingRename.to : name);
    state.pendingRename = null;
    changed = true;
  }
  if (state.pendingWorkspace && names.includes(state.pendingWorkspace.name)) {
    assignWorkspace(state.pendingWorkspace.name, state.pendingWorkspace.id);
    state.pendingWorkspace = null;
  }
  for (const item of state.workspaces) {
    const kept = item.sessions.filter((name) => names.includes(name));
    if (kept.length !== item.sessions.length) { item.sessions = kept; changed = true; }
  }
  if (changed) saveWorkspaces();
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

export function setGridSize(size) {
  state.gridSize = size;
  storage.set(GRID_KEY, size);
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

export function toggleUsage() {
  state.usageOpen = !state.usageOpen;
  storage.set(USAGE_KEY, state.usageOpen ? 'open' : null);
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

export function setRemote(on) {
  state.remote = on;
  storage.set(REMOTE_KEY, on ? 'on' : null);
}
