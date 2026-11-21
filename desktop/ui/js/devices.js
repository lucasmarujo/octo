import { activeDevice, forgetScoped, switchDevice } from './device.js';
import { ask, closeDialog, closeMenu, confirm, openDialog, openMenu } from './dialogs.js';
import { $, h, place, svg } from './dom.js';
import { plural, sessionMark } from './format.js';
import { t } from './i18n.js';
import { ICON_PLUS } from './icons.js';
import { toast } from './toast.js';
import { invoke } from './transport.js';

const POLL_MS = 3000;
const LINK_PLACEHOLDER = 'https://…/#k=…';
const TONES = { live: 'live', connecting: 'pending' };
const STOPPED = ['unpaired', 'outdated', 'replaced'];
const STEPS = [
  t('On the other computer, open Octo and click Pair phone (ctrl P)'),
  t('Turn it on and copy the link'),
  t('Paste the link here'),
];

let listing = { here: { agents: 0, working: 0, waiting: 0 }, devices: [] };
let painted = '';
let onListed = () => {};

export function deviceTone(phase) {
  return TONES[phase] || 'down';
}

export function deviceNote(device) {
  const phase = device.phase;
  if (phase === 'live') {
    const agents = plural(device.agents, t('agent'), t('agents'));
    return device.waiting ? `${agents} · ${t('{count} waiting for you', { count: device.waiting })}` : agents;
  }
  if (phase === 'connecting') return t('Connecting…');
  if (phase === 'unpaired') return t('This link stopped working · paste a new one');
  if (phase === 'outdated') return t('This Octo speaks another version · update both');
  if (phase === 'replaced') return t('Connected from somewhere else · try again');
  return t('Offline · trying again');
}

export function linkProblem(phase) {
  if (phase === 'unpaired') {
    return { title: t('This link stopped working'), text: t('The other Octo made a new pairing code. Copy its link again and paste it here.'), action: 'link' };
  }
  if (phase === 'outdated') {
    return { title: t('This Octo speaks another version'), text: t('Update Octo on both computers and try again.'), action: 'retry' };
  }
  if (phase === 'replaced') {
    return { title: t('Connected from somewhere else'), text: t('Another window took this connection.'), action: 'retry' };
  }
  if (phase === 'offline') {
    return { title: t('This device is offline'), text: t('Octo keeps trying. If the other Octo restarted its tunnel, paste its new link.'), action: 'link' };
  }
  return { title: t('Connecting…'), text: t('Opening an encrypted connection to the other Octo.'), action: null };
}

export function deviceError(error) {
  const text = String(error);
  if (text.includes('not a pairing link')) return t('That is not an Octo pairing link. Copy it from Pair phone on the other computer.');
  if (text.includes('forget one first')) return t('The list of devices is full. Forget one first.');
  return text;
}

export function currentDevice() {
  return listing.devices.find((device) => device.id === activeDevice()) || null;
}

async function refresh() {
  try {
    listing = await invoke('devices');
  } catch {
    return;
  }
  if (activeDevice() && !currentDevice()) {
    forgetScoped(activeDevice());
    switchDevice(null);
    return;
  }
  renderDevices();
  onListed();
}

function pick(id) {
  if (id !== activeDevice()) switchDevice(id);
}

async function rename(device) {
  const name = await ask({ title: t('Rename {name}', { name: device.name }), value: device.name, action: t('Rename'), placeholder: t('device name') });
  if (!name) return;
  await invoke('rename_device', { id: device.id, name }).catch((err) => toast(String(err), 'error'));
  refresh();
}

async function forget(device) {
  const sure = await confirm({
    title: t('Forget {name}?', { name: device.name }),
    text: t('Octo stops connecting to it. Its agents keep running there.'),
    action: t('Forget'),
    danger: true,
  });
  if (!sure) return;
  try {
    await invoke('forget_device', { id: device.id });
  } catch (err) {
    toast(String(err), 'error');
    return;
  }
  forgetScoped(device.id);
  if (device.id === activeDevice()) switchDevice(null);
  else refresh();
}

export function retryDevice(id) {
  invoke('retry_device', { id }).catch((err) => toast(String(err), 'error'));
}

function deviceMenu(event, device) {
  event.preventDefault();
  const item = (text, act) =>
    h(
      'button',
      {
        class: 'menu-item',
        type: 'button',
        role: 'menuitem',
        onclick: () => {
          closeMenu();
          act(device);
        },
      },
      h('span', null, text),
    );
  const items = [item(t('Rename'), rename), item(t('Paste a new link'), () => openAddDevice())];
  if (device.phase !== 'live') items.push(item(t('Try again'), () => retryDevice(device.id)));
  items.push(item(t('Forget'), forget));
  openMenu(event.currentTarget, items, false);
}

export function openAddDevice() {
  const field = h('input', { class: 'field', placeholder: LINK_PLACEHOLDER, spellcheck: 'false', 'aria-label': t('Pairing link') });
  let busy = false;
  const submit = async () => {
    const pairing = field.value.trim();
    if (!pairing || busy) return;
    busy = true;
    try {
      const device = await invoke('add_device', { pairing });
      closeDialog();
      switchDevice(device.id);
    } catch (err) {
      busy = false;
      toast(deviceError(err), 'error');
      field.focus();
    }
  };
  field.addEventListener('keydown', (event) => {
    if (event.key !== 'Enter') return;
    event.preventDefault();
    submit();
  });
  openDialog(
    t('Add a device'),
    [
      h('p', { class: 'dialog-text' }, t('Work on another computer running Octo from this window. End-to-end encrypted.')),
      h('ol', { class: 'pair-steps device-steps' }, ...STEPS.map((step) => h('li', null, step))),
      field,
    ],
    [
      h('button', { class: 'button', type: 'button', onclick: closeDialog }, t('Cancel')),
      h('button', { class: 'button primary', type: 'button', onclick: submit }, t('Connect')),
    ],
  );
  field.focus();
}

function row(id, name, tone, counts, note, device) {
  const selected = id === activeDevice();
  return h(
    'div',
    { class: `session${selected ? ' selected' : ''}` },
    h(
      'button',
      {
        class: 'session-row',
        type: 'button',
        title: note,
        'aria-current': selected ? 'true' : null,
        dataset: { device: id || '' },
        onclick: () => pick(id),
        oncontextmenu: device ? (event) => deviceMenu(event, device) : null,
      },
      h('span', { class: `device-dot ${tone}` }),
      h('span', { class: 'session-name' }, name),
      counts.waiting ? h('span', { class: 'session-hint device-waiting' }, String(counts.waiting)) : null,
      h('span', { class: 'session-count' }, tone === 'live' ? String(counts.agents) : ''),
    ),
  );
}

function chip(id, name, tone, counts, note, device) {
  const classes = ['rail-chip', `device-${tone}`, id === activeDevice() ? 'selected' : '', counts.waiting ? 'waiting' : ''];
  return h(
    'button',
    {
      class: classes.filter(Boolean).join(' '),
      type: 'button',
      'aria-label': `${name} · ${note}`,
      title: `${name} · ${note}`,
      dataset: { device: id || '' },
      onclick: () => pick(id),
      oncontextmenu: device ? (event) => deviceMenu(event, device) : null,
    },
    sessionMark(name),
  );
}

function entries() {
  const here = { ...listing.here, phase: 'live' };
  return [
    [null, t('This computer'), 'live', here, deviceNote(here), null],
    ...listing.devices.map((device) => [device.id, device.name, deviceTone(device.phase), device, deviceNote(device), device]),
  ];
}

export function renderDevices() {
  const signature = JSON.stringify([listing, activeDevice()]);
  if (signature === painted) return;
  painted = signature;
  const all = entries();
  const add = h(
    'button',
    { class: 'session-row new-session', type: 'button', onclick: openAddDevice },
    h('span', { class: 'chevron' }, svg(ICON_PLUS)),
    h('span', { class: 'session-name' }, t('Add a device')),
  );
  place($('devices'), [...all.map((entry) => row(...entry)), add]);
  const rail = $('device-rail');
  rail.hidden = !listing.devices.length;
  place(rail, all.map((entry) => chip(...entry)));
}

export function renderLinkProblem(phase) {
  const board = $('board');
  const device = currentDevice();
  const problem = linkProblem(phase);
  const signature = `link:${phase}:${device ? device.name : ''}`;
  if (board.dataset.signature === signature) return;
  board.dataset.signature = signature;
  const actions = [
    problem.action === 'link' ? h('button', { class: 'button primary', type: 'button', onclick: openAddDevice }, t('Paste a new link')) : null,
    problem.action ? h('button', { class: 'button', type: 'button', onclick: () => retryDevice(activeDevice()) }, t('Try again')) : null,
    h('button', { class: 'button', type: 'button', onclick: () => switchDevice(null) }, t('Back to this computer')),
  ];
  board.replaceChildren(
    h(
      'div',
      { class: 'empty' },
      h('span', { class: 'empty-title' }, device ? `${device.name} · ${problem.title}` : problem.title),
      h('span', null, problem.text),
      h('div', { class: 'empty-actions' }, actions),
    ),
  );
}

export function watchDevices(listed) {
  onListed = listed;
  renderDevices();
  refresh();
  setInterval(refresh, POLL_MS);
}
