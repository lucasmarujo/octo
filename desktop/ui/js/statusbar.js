import { $, h } from './dom.js';
import { SPINNER, USAGE_WARN, label, plural, usageColor } from './format.js';
import { language, t } from './i18n.js';
import { MODE_WORDS, octoStatus } from './orchestrator.js';
import { phoneNow } from './pair.js';
import { agents, state } from './state.js';

const BRAND = 'octo';
const PAUSED_MARK = '‖';

let actions = { attention: () => {}, phone: () => {}, updates: () => {} };

export function initStatusBar(handlers) {
  actions = handlers;
}

export function statusCounts(list) {
  return {
    total: list.length,
    working: list.filter((agent) => agent.starting || agent.state === 'working').length,
    needs: list.filter((agent) => !agent.starting && agent.state === 'paused').length,
  };
}

export function usageSummary(entries) {
  return (entries || []).filter((entry) => entry.windows && entry.windows.length).map((entry) => ({ kind: entry.kind, used: Math.round(entry.windows[0].used) }));
}

function phoneText(status) {
  return { off: t('phone off'), pending: t('phone starting…'), failed: t('phone needs attention'), ready: t('phone waiting'), connected: t('phone connected') }[status];
}

function build(counts, usage, phone, octo) {
  const version = state.updateReady
    ? h('button', { class: 'statusbar-item statusbar-ready', type: 'button', onclick: () => actions.updates() }, t('{version} ready · restart', { version: `v${state.updateReady}` }))
    : h('button', { class: 'statusbar-item statusbar-version', type: 'button', onclick: () => actions.updates() }, `v${state.version}`);
  return [
    h('span', { class: 'statusbar-brand' }, BRAND),
    h('span', { class: 'statusbar-item' }, plural(counts.total, t('agent'), t('agents'))),
    counts.working
      ? h('span', { class: 'statusbar-item statusbar-working' }, h('span', { class: 'glyph', dataset: { spin: '1' } }, SPINNER[state.frame % SPINNER.length]), t('{count} working', { count: counts.working }))
      : null,
    counts.needs
      ? h(
          'button',
          { class: 'statusbar-item statusbar-needs', type: 'button', title: t('alt N opens it'), onclick: () => actions.attention() },
          h('span', { class: 'glyph' }, PAUSED_MARK),
          counts.needs === 1 ? t('{count} needs you', { count: counts.needs }) : t('{count} need you', { count: counts.needs }),
        )
      : null,
    octo ? h('span', { class: 'statusbar-item' }, t('octo: {mode} · {count} handled today', { mode: MODE_WORDS[octo.mode], count: octo.handled })) : null,
    h('span', { class: 'grow' }),
    ...usage.map((entry) => h('span', { class: 'statusbar-item', style: entry.used >= USAGE_WARN ? `color:${usageColor(entry.used)}` : '' }, `${label(entry.kind)} ${entry.used}%`)),
    h('button', { class: `statusbar-item statusbar-phone ${phone}`, type: 'button', onclick: () => actions.phone() }, phoneText(phone)),
    state.version ? version : null,
  ].filter(Boolean);
}

export function renderStatusBar() {
  const host = $('statusbar');
  if (!host || !state.overview) return;
  const counts = statusCounts(agents());
  const usage = usageSummary(state.usage);
  const phone = phoneNow();
  const octo = octoStatus(state.orchestrator);
  const signature = JSON.stringify([counts, usage, phone, octo, state.version, state.updateReady, language()]);
  if (host.dataset.signature === signature) return;
  host.dataset.signature = signature;
  host.replaceChildren(...build(counts, usage, phone, octo));
}
