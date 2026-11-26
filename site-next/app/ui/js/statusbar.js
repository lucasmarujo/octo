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

function phoneTight(status) {
  return { off: t('off'), pending: '…', failed: '!', ready: t('wait'), connected: t('on') }[status];
}

function build(counts, usage, phone, octo) {
  const version = state.updateReady
    ? h(
        'button',
        { class: 'statusbar-item statusbar-ready statusbar-version-line', type: 'button', onclick: () => actions.updates() },
        h('span', { class: 'statusbar-wide' }, t('{version} ready · restart', { version: `v${state.updateReady}` })),
        h('span', { class: 'statusbar-tight' }, t('restart')),
      )
    : h('button', { class: 'statusbar-item statusbar-version', type: 'button', onclick: () => actions.updates() }, `v${state.version}`);
  const octoFull = octo ? t('octo: {mode} · {count} handled today', { mode: MODE_WORDS[octo.mode], count: octo.handled }) : '';
  const needsFull = counts.needs === 1 ? t('{count} needs you', { count: counts.needs }) : t('{count} need you', { count: counts.needs });
  return [
    h('span', { class: 'statusbar-brand' }, BRAND),
    h('span', { class: 'statusbar-item statusbar-total' }, plural(counts.total, t('agent'), t('agents'))),
    counts.working
      ? h(
          'span',
          { class: 'statusbar-item statusbar-working', title: t('{count} working', { count: counts.working }) },
          h('span', { class: 'glyph', dataset: { spin: '1' } }, SPINNER[state.frame % SPINNER.length]),
          h('span', { class: 'statusbar-wide' }, t('{count} working', { count: counts.working })),
          h('span', { class: 'statusbar-tight' }, String(counts.working)),
        )
      : null,
    counts.needs
      ? h(
          'button',
          { class: 'statusbar-item statusbar-needs', type: 'button', title: needsFull, onclick: () => actions.attention() },
          h('span', { class: 'glyph' }, PAUSED_MARK),
          h('span', { class: 'statusbar-wide' }, needsFull),
          h('span', { class: 'statusbar-tight' }, String(counts.needs)),
        )
      : null,
    octo
      ? h(
          'span',
          { class: 'statusbar-item statusbar-octo', title: octoFull },
          h('span', { class: 'statusbar-wide' }, octoFull),
          h('span', { class: 'statusbar-tight' }, t('{mode} · {count}', { mode: MODE_WORDS[octo.mode], count: octo.handled })),
        )
      : null,
    h('span', { class: 'grow' }),
    ...usage.map((entry, index) =>
      h(
        'span',
        {
          class: `statusbar-item statusbar-usage${index ? ' statusbar-usage-extra' : ''}`,
          style: entry.used >= USAGE_WARN ? `color:${usageColor(entry.used)}` : '',
          title: `${label(entry.kind)} ${entry.used}%`,
        },
        `${label(entry.kind)} ${entry.used}%`,
      ),
    ),
    h(
      'button',
      { class: `statusbar-item statusbar-phone ${phone}`, type: 'button', title: phoneText(phone), onclick: () => actions.phone() },
      h('span', { class: 'statusbar-wide' }, phoneText(phone)),
      h('span', { class: 'statusbar-tight' }, phoneTight(phone)),
    ),
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
