import { $, h, storage } from './dom.js';
import { joinArgs, kindColor, label, splitArgs } from './format.js';
import { LANGUAGES, language, setLanguage, t } from './i18n.js';
import { state } from './state.js';
import { toast } from './toast.js';
import { invoke } from './transport.js';

const THEME_KEY = 'orkai.theme';
const THEMES = [
  ['dark', t('Dark')],
  ['light', t('Light')],
  ['system', t('System')],
];
const TUNNELS = [
  ['cloudflare', 'Cloudflare'],
  ['tailscale', 'Tailscale'],
];
const FIRST_AGENTS = 4;
const CHIME_GAP_MS = 1300;
const SECTIONS = ['Appearance', 'Agents', 'Alerts', 'Phone', 'Updates'];

let current = null;
let showAll = false;
let update = null;

const commandKey = (kind) => `${kind === 'antigravity' ? 'agy' : kind}_command`;
const argsKey = (kind) => `${kind === 'antigravity' ? 'agy' : kind}_args`;
const mono = (text) => h('span', { class: 'mono-inline' }, text);

export function themeChoice() {
  const chosen = storage.get(THEME_KEY);
  return THEMES.some(([value]) => value === chosen) ? chosen : 'dark';
}

export function applyTheme() {
  const choice = themeChoice();
  const light = choice === 'light' || (choice === 'system' && window.matchMedia('(prefers-color-scheme: light)').matches);
  document.documentElement.dataset.theme = light ? 'light' : 'dark';
}

function setTheme(choice) {
  storage.set(THEME_KEY, choice === 'dark' ? null : choice);
  applyTheme();
  render();
}

function segmented(options, value, onPick, label) {
  return h(
    'div',
    { class: 'segmented', role: 'group', 'aria-label': label },
    ...options.map(([option, text]) =>
      h('button', { type: 'button', 'aria-pressed': String(option === value), onclick: () => option !== value && onPick(option) }, text),
    ),
  );
}

function toggle(on, onFlip, label) {
  return h('button', { class: 'switch', type: 'button', role: 'switch', 'aria-checked': String(on), 'aria-label': label, onclick: () => onFlip(!on) }, h('span', { class: 'switch-knob' }));
}

function field(value, placeholder, onSave, classes) {
  return h('input', {
    class: `field ${classes || ''}`,
    type: 'text',
    value,
    placeholder,
    spellcheck: 'false',
    onchange: (event) => onSave(event.target.value),
    onkeydown: (event) => event.key === 'Enter' && event.target.blur(),
  });
}

function row(title, description, control) {
  return h('div', { class: 'setting' }, h('div', { class: 'setting-text' }, h('span', { class: 'setting-title' }, title), h('span', { class: 'setting-description' }, description)), control);
}

function section(name, ...body) {
  return h('section', { class: 'settings-section', id: `settings-${name.toLowerCase()}` }, ...body);
}

async function save(changes) {
  try {
    current = await invoke('save_settings', { config: { ...current.config, ...changes } });
  } catch (err) {
    toast(String(err), 'error');
  }
  render();
}

function agentRow(agent) {
  const { config } = current;
  const status = agent.installed
    ? h('span', { class: 'agent-status' }, h('span', { class: 'status-dot on' }), t('On PATH'))
    : h('span', { class: 'agent-status t-faint' }, h('span', { class: 'status-dot' }), t('Not found'));
  return h(
    'div',
    { class: 'agents-row' },
    h('span', { class: 'agents-name', style: `color:${kindColor(agent.kind)}` }, label(agent.kind)),
    field(config[commandKey(agent.kind)], label(agent.kind), (value) => save({ [commandKey(agent.kind)]: value }), 'agents-command'),
    field(joinArgs(config[argsKey(agent.kind)]), t('No extra arguments'), (value) => save({ [argsKey(agent.kind)]: splitArgs(value) }), 'agents-args'),
    status,
  );
}

function agentsSection() {
  const agents = showAll ? current.agents : current.agents.slice(0, FIRST_AGENTS);
  const hidden = current.agents.slice(FIRST_AGENTS);
  const more = hidden.length
    ? h(
        'div',
        { class: 'agents-more' },
        h('button', { class: 'link-button', type: 'button', onclick: () => ((showAll = !showAll), render()) }, showAll ? t('Show fewer') : t('Show {count} more', { count: hidden.length })),
        showAll ? null : h('span', { class: 'agents-hidden' }, hidden.map((agent) => label(agent.kind)).join(' · ')),
      )
    : null;
  return section(
    'Agents',
    h(
      'div',
      { class: 'settings-heading with-text' },
      h('h2', null, t('Agents')),
      h('p', null, t('The program Octo runs for each agent and the extra arguments it passes. Only installed agents show up in'), ' ', mono(t('who')), '.'),
    ),
    h('div', { class: 'agents-head' }, h('span', { class: 'caps agents-name' }, t('Agent')), h('span', { class: 'caps agents-command' }, t('Command')), h('span', { class: 'caps agents-args' }, t('Arguments')), h('span', { class: 'caps agent-status' }, t('Status'))),
    ...agents.map(agentRow),
    more,
  );
}

async function playChimes() {
  try {
    const played = await invoke('play_chime', { sound: 'waiting' });
    if (!played) {
      toast(t('This computer cannot play the chime here.'), 'warn');
      return;
    }
    setTimeout(() => invoke('play_chime', { sound: 'done' }), CHIME_GAP_MS);
  } catch (err) {
    toast(String(err), 'error');
  }
}

async function checkUpdate() {
  update = { checking: true };
  render();
  try {
    update = await invoke('check_update');
    if (update.ready) state.updateReady = update.latest;
  } catch (err) {
    update = { failed: true };
    toast(t('Could not check for updates: {error}', { error: err }), 'warn');
  }
  render();
}

function updateLine() {
  const version = `Octo ${state.version || ''}`.trim();
  if (state.updateReady) return [version, h('span', { class: 'update-note t-green' }, ` · ${t('{version} is ready, restart to use it', { version: state.updateReady })}`)];
  if (!update) return [version];
  if (update.checking) return [version, h('span', { class: 'update-note t-muted' }, ` · ${t('checking…')}`)];
  if (update.failed) return [version, h('span', { class: 'update-note t-muted' }, ` · ${t('could not check')}`)];
  if (update.newer) return [version, h('span', { class: 'update-note t-yellow' }, ` · ${t('{version} available', { version: update.latest })}`)];
  return [version, h('span', { class: 'update-note t-green' }, ` · ${t('up to date')}`)];
}

function content() {
  const { config } = current;
  return h(
    'div',
    { class: 'settings-content' },
    section(
      'Appearance',
      h('div', { class: 'settings-heading' }, h('h2', null, t('Appearance'))),
      row(t('Theme'), t('The agent terminals follow the theme too.'), segmented(THEMES, themeChoice(), setTheme, t('Theme'))),
      row(t('Language'), t('English by default. Applies to the whole app.'), segmented(LANGUAGES.map((entry) => [entry.code, entry.name]), language(), (code) => setLanguage(code, 'settings'), t('Language'))),
      row(
        t('Terminal shell'),
        [t('Used by terminals and'), ' ', mono('!command'), '. ', t('Empty uses {rule}.', { rule: t(current.shellRule) })],
        field(config.shell || '', current.defaultShell, (value) => save({ shell: value.trim() || null }), 'wide'),
      ),
    ),
    agentsSection(),
    section(
      'Alerts',
      h('div', { class: 'settings-heading' }, h('h2', null, t('Alerts'))),
      row(t('Alert when an agent needs you or finishes'), t('Sound, desktop notification and the count in the taskbar title.'), toggle(config.notify, (on) => save({ notify: on }), t('Alerts'))),
      row(
        t('Octo chime'),
        t('Falling tone when an agent waits for you, rising when it finishes. Off uses the system bell.'),
        h('div', { class: 'setting-controls' }, h('button', { class: 'link-button', type: 'button', onclick: playChimes }, t('Play')), toggle(config.notify_sound, (on) => save({ notify_sound: on }), t('Octo chime'))),
      ),
      h(
        'div',
        { class: 'setting stacked' },
        h(
          'div',
          { class: 'setting-text' },
          h('span', { class: 'setting-title' }, t('Notify command')),
          h('span', { class: 'setting-description' }, t('Runs on every alert, without a shell, with'), ' ', mono('ORKAI_MESSAGE'), ', ', mono('ORKAI_AGENT_NAME'), ' ', t('and more. Killed after 30 s.')),
        ),
        h(
          'div',
          { class: 'setting-fields' },
          field(config.notify_command || '', t('Program'), (value) => save({ notify_command: value.trim() || null }), 'narrow'),
          field(joinArgs(config.notify_args), t('Arguments'), (value) => save({ notify_args: splitArgs(value) }), 'grow'),
        ),
      ),
    ),
    section(
      'Phone',
      h('div', { class: 'settings-heading' }, h('h2', null, t('Phone'))),
      row(t('Tunnel'), t('How the phone reaches this computer. End-to-end encrypted either way.'), segmented(TUNNELS, config.pwa_tunnel, (tunnel) => save({ pwa_tunnel: tunnel }), t('Tunnel'))),
      row(t('Local port'), t('The bridge only listens on 127.0.0.1.'), field(String(config.pwa_port), '7470', (value) => save({ pwa_port: Number(value) || 0 }), 'narrow')),
    ),
    section(
      'Updates',
      h('div', { class: 'settings-heading' }, h('h2', null, t('Updates'))),
      row(
        h('span', null, ...updateLine()),
        t('Updates install on their own, checked by size and SHA-256. Running agents are never stopped.'),
        state.updateReady
          ? h('button', { class: 'button primary', type: 'button', onclick: () => invoke('restart_app') }, t('Restart now'))
          : h('button', { class: 'button', type: 'button', disabled: Boolean(update && update.checking), onclick: checkUpdate }, t('Check now')),
      ),
    ),
  );
}

function scrollTo(name) {
  const target = document.getElementById(`settings-${name.toLowerCase()}`);
  if (target) target.scrollIntoView({ behavior: 'smooth', block: 'start' });
}

function markSection() {
  const host = $('settings-view');
  const top = host.getBoundingClientRect().top;
  let active = SECTIONS[0];
  for (const name of SECTIONS) {
    const node = document.getElementById(`settings-${name.toLowerCase()}`);
    if (node && node.getBoundingClientRect().top - top < host.clientHeight / 3) active = name;
  }
  if (host.scrollTop + host.clientHeight >= host.scrollHeight - 2) active = SECTIONS.at(-1);
  for (const item of host.querySelectorAll('.settings-nav-item')) item.setAttribute('aria-current', String(item.dataset.section === active));
}

function nav() {
  return h(
    'nav',
    { class: 'settings-nav' },
    h('h1', { class: 'settings-title' }, t('Settings')),
    ...SECTIONS.map((name) => h('button', { class: 'settings-nav-item', type: 'button', dataset: { section: name }, onclick: () => scrollTo(name) }, t(name))),
    h('div', { class: 'settings-saved' }, h('span', null, t('Saved to')), h('span', { class: 'settings-path' }, current.path.replace(/([\\/])/g, '$1\u200b'))),
  );
}

function render() {
  const host = $('settings-view');
  if (!current) return;
  const scroll = host.scrollTop;
  host.replaceChildren(h('div', { class: 'settings' }, nav(), content()));
  host.scrollTop = scroll;
  markSection();
}

export async function renderSettings() {
  const host = $('settings-view');
  if (!host.dataset.ready) {
    host.addEventListener('scroll', markSection, { passive: true });
    host.dataset.ready = '1';
  }
  try {
    current = await invoke('settings');
    render();
  } catch (err) {
    toast(t('Could not read the settings: {error}', { error: err }), 'error');
  }
}
