import { $, h, storage, svg } from './dom.js';
import { confirm } from './dialogs.js';
import { alive, installKind, joinArgs, kindColor, label, soundFlip, soundOn, splitArgs, volumeChanges } from './format.js';
import { LANGUAGES, language, setLanguage, t } from './i18n.js';
import { ICON_BELL, ICON_BELL_OFF } from './icons.js';
import { choose, hideTip, motion, reducedMotion, rise, settle, slideThumb } from './motion.js';
import { NOTES, noteBlock } from './notes.js';
import { agents, state } from './state.js';
import { toast } from './toast.js';
import { invoke } from './transport.js';

const THEME_KEY = 'orkai.theme';
const THEMES = [
  ['dark', t('Dark')],
  ['gray', t('Gray')],
  ['light', t('Light')],
  ['system', t('System')],
];
const TUNNELS = [
  ['cloudflare', 'Cloudflare'],
  ['tailscale', 'Tailscale'],
];
const CHIME_GAP_MS = 1300;
const PREVIEW_DELAY_MS = 150;
const POPOVER_CLOSE_MS = 220;
const POPOVER_GAP = 6;
const POPOVER_MARGIN = 8;

let current = null;
let acked = null;
let saves = 0;
let dragging = false;
let hovering = false;
let dismissed = false;
let closeTimer = 0;
let previewTimer = 0;
let liveInstalls = new Map();
let openAgent = () => {};
const pendingInstalls = new Map();
let shownSection = 'Appearance';
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
  const system = window.matchMedia('(prefers-color-scheme: light)').matches ? 'light' : 'dark';
  document.documentElement.dataset.theme = choice === 'system' ? system : choice;
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
      h('button', { type: 'button', 'aria-pressed': String(option === value), onclick: (event) => option !== value && choose(event.currentTarget, () => onPick(option)) }, text),
    ),
  );
}

function toggle(on, onFlip, label) {
  const flip = (event) => {
    event.currentTarget.setAttribute('aria-checked', String(!on));
    settle(() => onFlip(!on));
  };
  return h('button', { class: 'switch', type: 'button', role: 'switch', 'aria-checked': String(on), 'aria-label': label, onclick: flip }, h('span', { class: 'switch-knob' }));
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

function section(...body) {
  return h('section', { class: 'settings-section' }, ...body);
}

function renderSound() {
  const on = !current || soundOn(current.config);
  const button = $('sound');
  const name = on ? t('Sound on') : t('Sound off');
  button.replaceChildren(svg(on ? ICON_BELL : ICON_BELL_OFF));
  button.setAttribute('aria-pressed', String(on));
  button.setAttribute('aria-label', name);
  button.title = name;
  paintVolume();
}

function paintVolume() {
  if (!current) return;
  const volume = current.config.notify_volume;
  const off = !soundOn(current.config);
  for (const slider of document.querySelectorAll('.volume-slider')) {
    slider.value = String(volume);
    slider.style.setProperty('--volume', `${volume}%`);
    slider.classList.toggle('off', off);
    slider.setAttribute('aria-valuetext', `${volume}%`);
  }
  for (const value of document.querySelectorAll('.volume-value')) value.textContent = `${volume}%`;
}

async function save(changes) {
  const turn = ++saves;
  current = { ...current, config: { ...current.config, ...changes } };
  renderSound();
  try {
    const saved = await invoke('save_settings', { config: current.config });
    acked = saved;
    if (turn === saves && !dragging) current = saved;
  } catch (err) {
    toast(String(err), 'error');
    if (turn === saves) current = acked;
  }
  renderSound();
  render();
}

function flipSound() {
  if (!current) return;
  save(soundFlip(current.config));
  render();
}

function moveVolume(volume) {
  if (!current) return;
  current = { ...current, config: { ...current.config, ...volumeChanges(current.config, volume) } };
  renderSound();
}

function previewChime() {
  clearTimeout(previewTimer);
  previewTimer = setTimeout(() => invoke('play_chime', { sound: 'waiting' }).catch(() => {}), PREVIEW_DELAY_MS);
}

async function commitVolume(volume) {
  if (!current) return;
  dragging = false;
  await save(volumeChanges(current.config, volume));
  if (soundOn(current.config) && current.config.notify_volume === volume) previewChime();
}

function bindVolume(slider) {
  slider.addEventListener('pointerdown', () => {
    dragging = true;
  });
  slider.addEventListener('input', () => moveVolume(Number(slider.value)));
  slider.addEventListener('change', () => commitVolume(Number(slider.value)));
  return slider;
}

function volumeControl() {
  const slider = bindVolume(h('input', { class: 'volume-slider', type: 'range', min: '0', max: '100', step: '1', 'aria-label': t('Volume') }));
  return h('div', { class: 'volume-control' }, slider, h('span', { class: 'volume-value' }));
}

function usingKeyboard() {
  return $('sound').matches(':focus-visible') || $('sound-volume').matches(':focus-visible');
}

function placePopover() {
  const box = $('sound').getBoundingClientRect();
  const popover = $('sound-popover');
  const beside = document.documentElement.dataset.sidebar === 'collapsed';
  const left = beside ? box.right + POPOVER_GAP : box.left + box.width / 2 - popover.offsetWidth / 2;
  const top = beside ? box.top + box.height / 2 - popover.offsetHeight / 2 : box.top - popover.offsetHeight - POPOVER_GAP;
  popover.style.left = `${Math.max(POPOVER_MARGIN, Math.min(left, window.innerWidth - popover.offsetWidth - POPOVER_MARGIN))}px`;
  popover.style.top = `${Math.max(POPOVER_MARGIN, Math.min(top, window.innerHeight - popover.offsetHeight - POPOVER_MARGIN))}px`;
}

function openPopover() {
  clearTimeout(closeTimer);
  const popover = $('sound-popover');
  if (!popover.hidden || dismissed) return;
  hideTip();
  $('sound').setAttribute('data-tip-off', '');
  popover.hidden = false;
  placePopover();
}

function closePopover() {
  clearTimeout(closeTimer);
  $('sound-popover').hidden = true;
  $('sound').removeAttribute('data-tip-off');
}

function closeSoon() {
  clearTimeout(closeTimer);
  closeTimer = setTimeout(() => {
    if (!hovering && !dragging && !usingKeyboard()) closePopover();
  }, POPOVER_CLOSE_MS);
}

function initPopover() {
  const bell = $('sound');
  const popover = $('sound-popover');
  const enter = () => {
    hovering = true;
    dismissed = false;
    openPopover();
  };
  const leave = () => {
    hovering = false;
    closeSoon();
  };
  for (const part of [bell, popover]) {
    part.addEventListener('pointerenter', enter);
    part.addEventListener('pointerleave', leave);
    part.addEventListener('focusout', (event) => {
      if (event.relatedTarget === bell || popover.contains(event.relatedTarget)) return;
      dismissed = false;
      if (!hovering && !dragging) closePopover();
    });
  }
  bell.addEventListener('focusin', () => {
    if (bell.matches(':focus-visible')) openPopover();
  });
  bindVolume($('sound-volume'));
  window.addEventListener('pointerup', () => {
    dragging = false;
    if (!hovering && !popover.hidden) closeSoon();
  });
  window.addEventListener('resize', () => {
    if (!popover.hidden) placePopover();
  });
  document.addEventListener('keydown', (event) => {
    if (event.key !== 'Escape' || popover.hidden) return;
    dismissed = true;
    const focusedInside = popover.contains(document.activeElement);
    closePopover();
    if (focusedInside) bell.focus();
  });
}

const installing = (kind) => pendingInstalls.has(kind) || agents().some((agent) => alive(agent) && installKind(agent) === kind);

export function setAgentOpener(open) {
  openAgent = open;
}

async function recheck() {
  try {
    const statuses = await invoke('recheck_agents');
    if (current && JSON.stringify(statuses) !== JSON.stringify(current.agents)) {
      current = { ...current, agents: statuses };
      acked = { ...acked, agents: statuses };
      render();
    }
    return statuses;
  } catch {
    return null;
  }
}

async function installsEnded(kinds) {
  const statuses = await recheck();
  if (!statuses) return;
  for (const kind of kinds) {
    const name = label(kind);
    if (statuses.some((agent) => agent.kind === kind && agent.installed)) toast(t('{name} is installed.', { name }), 'success');
    else toast(t('{name} is still not on PATH. If the install worked, check that its folder is on PATH and restart Octo.', { name }), 'warn');
  }
}

export function watchInstalls(list) {
  for (const [kind, id] of pendingInstalls) {
    const agent = list.find((item) => item.id === id);
    if (!agent) continue;
    pendingInstalls.delete(kind);
    liveInstalls.set(id, kind);
    openAgent(agent);
  }
  const live = new Map(list.filter((agent) => alive(agent) && installKind(agent)).map((agent) => [agent.id, installKind(agent)]));
  const ended = [...liveInstalls].filter(([id]) => !live.has(id)).map(([, kind]) => kind);
  const changed = ended.length > 0 || live.size !== liveInstalls.size;
  liveInstalls = live;
  if (changed) render();
  if (ended.length) installsEnded(ended);
}

async function install(agent) {
  const sure = await confirm({
    title: t('Install {name}?', { name: label(agent.kind) }),
    text: t('This downloads and runs software from the vendor.'),
    code: agent.install,
    action: t('Install'),
  });
  if (!sure) return;
  pendingInstalls.set(agent.kind, null);
  render();
  try {
    pendingInstalls.set(agent.kind, await invoke('install_agent', { kind: agent.kind, session: state.session }));
    watchInstalls(agents());
  } catch (err) {
    pendingInstalls.delete(agent.kind);
    toast(String(err), 'error');
    render();
  }
}

function installCell(agent) {
  if (agent.installed || !agent.install) return h('span', { class: 'agent-install' });
  const busy = installing(agent.kind);
  return h('span', { class: 'agent-install' }, h('button', { class: 'button tiny', type: 'button', disabled: busy, onclick: () => install(agent) }, busy ? t('Installing…') : t('Install')));
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
    installCell(agent),
  );
}

function agentsSection() {
  return section(
    h(
      'div',
      { class: 'settings-heading with-text' },
      h('h2', null, t('Agents')),
      h('p', null, t('The program Octo runs for each agent and the extra arguments it passes. Only installed agents can be picked in the task box.')),
    ),
    h('div', { class: 'agents-head' }, h('span', { class: 'caps agents-name' }, t('Agent')), h('span', { class: 'caps agents-command' }, t('Command')), h('span', { class: 'caps agents-args' }, t('Arguments')), h('span', { class: 'caps agent-status' }, t('Status')), h('span', { class: 'agent-install' })),
    ...current.agents.map(agentRow),
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
    toast(t('Could not check for updates: {error}', { error: t(String(err)) }), 'warn');
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

function appearanceSection() {
  const { config } = current;
  return section(
    h('div', { class: 'settings-heading' }, h('h2', null, t('Appearance'))),
    row(t('Theme'), t('The agent terminals follow the theme too.'), segmented(THEMES, themeChoice(), setTheme, t('Theme'))),
    row(t('Language'), t('English by default. Applies to the whole app.'), segmented(LANGUAGES.map((entry) => [entry.code, entry.name]), language(), (code) => setLanguage(code, 'settings'), t('Language'))),
    row(
      t('Terminal shell'),
      [t('Used by terminals and'), ' ', mono('!command'), '. ', t('Empty uses {rule}.', { rule: t(current.shellRule) })],
      field(config.shell || '', current.defaultShell, (value) => save({ shell: value.trim() || null }), 'wide'),
    ),
  );
}

function alertsSection() {
  const { config } = current;
  return section(
    h('div', { class: 'settings-heading' }, h('h2', null, t('Alerts'))),
    row(t('Alert when an agent needs you or finishes'), t('Sound, desktop notification and the count in the taskbar title.'), toggle(config.notify, (on) => save({ notify: on }), t('Alerts'))),
    row(
      t('Octo chime'),
      t('Falling tone when an agent waits for you, rising when it finishes. Off plays no sound.'),
      h('div', { class: 'setting-controls' }, h('button', { class: 'link-button', type: 'button', onclick: playChimes }, t('Play')), toggle(config.notify_sound, (on) => save({ notify_sound: on }), t('Octo chime'))),
    ),
    row(t('Volume'), soundOn(config) ? t('How loud the chime plays. The volume of the system stays as it is.') : t('The sound is off. Moving this turns it back on.'), volumeControl()),
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
  );
}

function phoneSection() {
  const { config } = current;
  return section(
    h('div', { class: 'settings-heading' }, h('h2', null, t('Phone'))),
    row(t('Tunnel'), t('How the phone reaches this computer. End-to-end encrypted either way.'), segmented(TUNNELS, config.pwa_tunnel, (tunnel) => save({ pwa_tunnel: tunnel }), t('Tunnel'))),
    row(t('Local port'), t('The bridge only listens on 127.0.0.1.'), field(String(config.pwa_port), '7470', (value) => save({ pwa_port: Number(value) || 0 }), 'narrow')),
  );
}

function updatesSection() {
  return section(
    h('div', { class: 'settings-heading' }, h('h2', null, t('Updates'))),
    current.storeUpdates
      ? row(`Octo ${state.version || ''}`.trim(), t('The Microsoft Store installs the updates. Running agents are never stopped.'), null)
      : row(
          h('span', null, ...updateLine()),
          t('Updates install on their own, checked by size and SHA-256. Running agents are never stopped.'),
          state.updateReady
            ? h('button', { class: 'button primary', type: 'button', onclick: () => invoke('restart_app') }, t('Restart now'))
            : h('button', { class: 'button', type: 'button', disabled: Boolean(update && update.checking), onclick: checkUpdate }, t('Check now')),
        ),
    h('div', { class: 'settings-heading notes-heading' }, h('h2', null, t('Release notes'))),
    h('div', { class: 'notes' }, NOTES.map((note) => noteBlock(note, note.version === state.version))),
  );
}

const SECTIONS = { Appearance: appearanceSection, Agents: agentsSection, Alerts: alertsSection, Phone: phoneSection, Updates: updatesSection };

export function pickSection(name) {
  shownSection = name;
}

function showSection(name) {
  if (name === shownSection) return;
  shownSection = name;
  const host = $('settings-view');
  render();
  host.scrollTop = 0;
  if (!reducedMotion()) rise(host.querySelector('.settings-section'), motion('--motion-base'));
}

function nav() {
  return h(
    'nav',
    { class: 'settings-nav' },
    h('h1', { class: 'settings-title' }, t('Settings')),
    ...Object.keys(SECTIONS).map((name) => h('button', { class: 'settings-nav-item', type: 'button', 'aria-current': String(name === shownSection), onclick: () => showSection(name) }, t(name))),
    h('div', { class: 'settings-saved' }, h('span', null, t('Saved to')), h('span', { class: 'settings-path' }, current.path.replace(/([\\/])/g, '$1\u200b'))),
  );
}

function render() {
  const host = $('settings-view');
  if (!current || dragging) return;
  const scroll = host.scrollTop;
  const onSlider = host.contains(document.activeElement) && document.activeElement.matches('.volume-slider');
  host.replaceChildren(h('div', { class: 'settings' }, nav(), h('div', { class: 'settings-content' }, SECTIONS[shownSection]())));
  host.scrollTop = scroll;
  for (const group of host.querySelectorAll('.segmented')) slideThumb(group);
  paintVolume();
  if (onSlider) host.querySelector('.volume-slider')?.focus();
}

async function load() {
  current = await invoke('settings');
  acked = current;
  renderSound();
}

export function initSound() {
  renderSound();
  $('sound').addEventListener('click', flipSound);
  initPopover();
  load().catch((err) => toast(t('Could not read the settings: {error}', { error: err }), 'error'));
}

export async function renderSettings() {
  try {
    await load();
    render();
    recheck();
  } catch (err) {
    toast(t('Could not read the settings: {error}', { error: err }), 'error');
  }
}
