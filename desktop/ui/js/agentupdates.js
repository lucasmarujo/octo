import { $, h, storage, svg } from './dom.js';
import { alive, versionAtLeast } from './format.js';
import { t } from './i18n.js';
import { ICON_TERMINAL } from './icons.js';
import { motionMs } from './motion.js';
import { findAgent, state } from './state.js';
import { toast } from './toast.js';
import { invoke, send } from './transport.js';

const DISMISSED_KEY = 'octo.agentUpdates';
const UPDATED_MS = 5000;

const cards = new Map();
let openAgent = () => {};

export const agentName = (kind) => kind.charAt(0).toUpperCase() + kind.slice(1);

export function dismissedFor(skipped, update) {
  return skipped[update.kind] === update.latest;
}

function close(card, remember) {
  if (cards.get(card.update.kind) !== card) return;
  cards.delete(card.update.kind);
  if (remember) storage.set(DISMISSED_KEY, JSON.stringify({ ...storage.json(DISMISSED_KEY, {}), [card.update.kind]: card.update.latest }));
  card.node.classList.add('leaving');
  setTimeout(() => card.node.remove(), motionMs('--motion-base'));
}

function titleOf(card) {
  const name = agentName(card.update.kind);
  if (card.phase === 'updating') return t('Updating {name}…', { name });
  if (card.phase === 'updated') return t('{name} updated', { name });
  if (card.phase === 'failed') return t('{name} was not updated', { name });
  return t('{name} update available', { name });
}

function detailOf(card) {
  if (card.phase === 'updated') return `v${card.version}`;
  if (card.phase === 'failed') return t('The update terminal shows why.');
  return `v${card.update.installed} → v${card.update.latest}`;
}

function actionOf(card) {
  if (card.phase === 'available') return h('button', { class: 'button', type: 'button', onclick: () => start(card) }, t('Update'));
  if (card.phase === 'updating') return h('span', { class: 'agent-update-spin', 'aria-hidden': 'true' });
  if (card.phase === 'failed' && findAgent(card.agent)) return h('button', { class: 'button', type: 'button', onclick: () => openAgent(findAgent(card.agent)) }, t('Open terminal'));
  return null;
}

function paint(card) {
  card.node.dataset.phase = card.phase;
  const parts = [
    h('span', { class: 'agent-update-icon' }, svg(ICON_TERMINAL)),
    h('div', { class: 'agent-update-text' }, h('span', { class: 'agent-update-title' }, titleOf(card)), h('span', { class: 'agent-update-detail' }, detailOf(card))),
    actionOf(card),
    h('button', { class: 'agent-update-close', type: 'button', 'aria-label': t('Dismiss'), title: t('Dismiss'), onclick: () => close(card, card.phase === 'available') }, '×'),
  ];
  card.node.replaceChildren(...parts.filter(Boolean));
}

async function start(card) {
  card.phase = 'updating';
  paint(card);
  try {
    card.agent = await invoke('update_agent', { kind: card.update.kind, session: state.session });
  } catch (err) {
    card.phase = 'available';
    paint(card);
    toast(String(err), 'error');
  }
}

async function finish(card, agent) {
  card.ending = true;
  const version = await invoke('agent_version', { kind: card.update.kind }).catch(() => null);
  if (!versionAtLeast(version, card.update.latest)) {
    card.phase = 'failed';
    paint(card);
    return;
  }
  card.phase = 'updated';
  card.version = version;
  paint(card);
  if (agent) send({ type: 'remove', agent: agent.id });
  setTimeout(() => close(card, false), UPDATED_MS);
}

export function watchAgentUpdates(list) {
  for (const card of cards.values()) {
    if (card.phase !== 'updating' || card.agent === null || card.ending) continue;
    const agent = list.find((item) => item.id === card.agent);
    if (agent && alive(agent)) {
      card.seen = true;
      continue;
    }
    if (agent || card.seen) finish(card, agent);
  }
}

export async function checkAgentUpdates(open) {
  openAgent = open;
  let updates;
  try {
    updates = await invoke('agent_updates');
  } catch {
    return;
  }
  const skipped = storage.json(DISMISSED_KEY, {});
  for (const update of updates) {
    if (dismissedFor(skipped, update) || cards.has(update.kind)) continue;
    const card = { update, phase: 'available', agent: null, seen: false, ending: false, version: null, node: h('div', { class: 'toast agent-update', role: 'status' }) };
    cards.set(update.kind, card);
    paint(card);
    $('toasts').prepend(card.node);
  }
}
