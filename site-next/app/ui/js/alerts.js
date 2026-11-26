import { t } from './i18n.js';

export const ALERT_REPEAT_MS = 15_000;
export const APP_TITLE = 'Octo';

const ai = (agent) => agent.kind !== 'terminal' && !agent.starting;

export function detectAlerts(previous, agents, viewing, now, lastAlerts) {
  const alerts = [];
  const states = new Map();
  for (const agent of agents) {
    states.set(agent.id, agent.state);
    const before = previous.get(agent.id);
    if (!ai(agent) || before === undefined || agent.id === viewing) continue;
    const finished = agent.state === 'done' && before !== 'done';
    const needsYou = agent.state === 'paused' && before !== 'paused';
    if (!finished && !needsYou) continue;
    const last = lastAlerts.get(agent.id);
    if (last && last.state === agent.state && now - last.at < ALERT_REPEAT_MS) continue;
    lastAlerts.set(agent.id, { state: agent.state, at: now });
    alerts.push({
      id: agent.id,
      name: agent.name,
      kind: agent.kind,
      state: agent.state,
      session: agent.session,
      cwd: agent.cwd,
      activity: agent.activity,
    });
  }
  return { alerts, states };
}

export function windowTitle(agents) {
  const waiting = agents.filter((agent) => ai(agent) && agent.state === 'paused').length;
  const done = agents.filter((agent) => ai(agent) && agent.state === 'done').length;
  const parts = [waiting ? `‖${waiting}` : '', done ? `✔${done}` : ''].filter(Boolean);
  return parts.length ? `${parts.join(' ')} · ${APP_TITLE}` : APP_TITLE;
}

export function notice(alert) {
  return alert.state === 'paused'
    ? { text: t('{name} is waiting for you · alt+n opens it', { name: alert.name }), tone: 'warn' }
    : { text: t('{name} finished · alt+n opens it', { name: alert.name }), tone: 'success' };
}
