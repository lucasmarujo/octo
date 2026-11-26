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

export function islandShape(pill, panel, notch, open, { edge, offset }, viewport) {
  const rect = (size) => {
    const width = Math.min(size.width, viewport.width);
    const height = Math.min(size.height, viewport.height);
    const left = viewport.width - width;
    const top = viewport.height - height;
    const x = edge === 'left' ? 0 : edge === 'right' ? left : Math.min(left, Math.max(0, offset - width / 2));
    const y = edge === 'top' ? 0 : edge === 'bottom' ? top : Math.min(top, Math.max(0, offset - height / 2));
    return { x, y, width, height };
  };
  const collapsed = rect(pill);
  const expanded = rect(notch
    ? { width: Math.max(panel.width, pill.width), height: panel.height + notch.height }
    : panel);
  return { collapsed, expanded, shown: open ? expanded : collapsed };
}
