import { column, groupByProject } from './format.js';

export const ISLAND_MAX_ROWS = 30;

const ORDER = ['needs', 'working', 'done', 'idle'];
const rank = (agent) => ORDER.indexOf(column(agent));

export function islandAgents(agents) {
  return agents.filter((agent) => agent.state !== 'exited');
}

export function islandCounts(agents) {
  const counts = { total: agents.length, needs: 0, working: 0, done: 0, idle: 0 };
  for (const agent of agents) counts[column(agent)] += 1;
  return counts;
}

export function islandGroups(agents, limit = ISLAND_MAX_ROWS) {
  const sorted = [...agents].sort((a, b) => rank(a) - rank(b));
  const shown = sorted.slice(0, limit);
  const groups = groupByProject(shown).sort((a, b) => rank(a.agents[0]) - rank(b.agents[0]));
  return { groups, more: sorted.length - shown.length, projects: new Set(agents.map((agent) => agent.project)).size };
}

export function islandShape(pill, panel, notch, open) {
  const collapsed = { width: pill.width, height: pill.height };
  const expanded = notch
    ? { width: Math.max(panel.width, pill.width), height: panel.height + notch.height }
    : { width: panel.width, height: panel.height };
  return { collapsed, expanded, shown: open ? expanded : collapsed };
}
