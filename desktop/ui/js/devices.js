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
const SSH_PORT = 22;
const MAX_PORT = 65535;
const TONES = { live: 'live', connecting: 'pending', installing: 'pending' };
const STOPPED = ['unpaired', 'outdated', 'replaced', 'password', 'unsupported', 'changed'];
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

export function linkStopped(phase) {
  return STOPPED.includes(phase);
}

export function deviceNote(device) {
  const phase = device.phase;
  if (phase === 'live') {
    const agents = plural(device.agents, t('agent'), t('agents'));
    return device.waiting ? `${agents} · ${t('{count} waiting for you', { count: device.waiting })}` : agents;
  }
  if (phase === 'connecting') return t('Connecting…');
  if (phase === 'installing') return t('Installing Octo on the server…');
  if (phase === 'password') return t('Needs the password');
  if (phase === 'unsupported') return t('This server is not supported');
  if (phase === 'changed') return t('The key of this server changed');
  if (phase === 'unpaired') return t('This link stopped working · paste a new one');
  if (phase === 'outdated') return t('This Octo speaks another version · update both');
  if (phase === 'replaced') return t('Connected from somewhere else · try again');
  return t('Offline · trying again');
}

export function linkProblem(phase, kind = 'link') {
  if (phase === 'password') {
    return { title: t('This server asks for a password'), text: t('The password stays in memory only and is never saved. Octo asks again after it restarts; an SSH key or agent never asks.'), action: 'password' };
  }
  if (phase === 'installing') {
    return { title: t('Installing Octo on the server'), text: t('Octo is downloading its server into ~/.octo/bin there and checking its SHA-256 before running it.'), action: null };
  }
  if (phase === 'unsupported') {
    return { title: t('This server is not supported'), text: t('Octo runs on Linux servers, x86_64 or arm64, for now.'), action: 'retry' };
  }
  if (phase === 'changed') {
    return { title: t('The key of this server changed'), text: t('If the server was reinstalled, remove its old key from known_hosts on this computer and try again.'), action: 'retry' };
  }
  if (phase === 'unpaired') {
    return { title: t('This link stopped working'), text: t('The other Octo made a new pairing code. Copy its link again and paste it here.'), action: 'link' };
  }
  if (phase === 'outdated') {
    return { title: t('This Octo speaks another version'), text: t('Update Octo on both computers and try again.'), action: 'retry' };
  }
  if (phase === 'replaced') {
    return { title: t('Connected from somewhere else'), text: t('Another window took this connection.'), action: 'retry' };
  }
  if (phase === 'offline' && kind === 'ssh') {
    return { title: t('This server is offline'), text: t('Octo keeps trying to reach it over SSH.'), action: 'retry' };
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
  if (text.includes('not a host name')) return t('That is not a host name or an address.');
  if (text.includes('not a user name')) return t('That is not a user name.');
  if (text.includes('not a port')) return t('That is not a port.');
  return text;
}

export function serverFields(host, port, user) {
  const number = String(port).trim() === '' ? SSH_PORT : Number(port);
  if (!host.trim() || !user.trim()) return null;
  if (!Number.isInteger(number) || number < 1 || number > MAX_PORT) return null;
  return { host: host.trim(), port: number, user: user.trim() };
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
    text: device.kind === 'ssh'
      ? t('Octo closes the SSH connection. Its agents keep running there, and ~/.octo/bin stays on the server until you delete it.')
      : t('Octo stops connecting to it. Its agents keep running there.'),
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

function unlock(id, field) {
  const password = field.value;
  if (!password) return;
  field.value = '';
  invoke('unlock_device', { id, password }).catch((err) => toast(String(err), 'error'));
}

function passwordField() {
  return h('input', { class: 'field', type: 'password', autocomplete: 'off', spellcheck: 'false', placeholder: t('password'), 'aria-label': t('Password') });
}

async function askPassword(device) {
  const field = passwordField();
  const submit = () => {
    if (!field.value) return;
    unlock(device.id, field);
    closeDialog();
  };
  field.addEventListener('keydown', (event) => {
    if (event.key !== 'Enter') return;
    event.preventDefault();
    submit();
  });
  openDialog(
    t('Password of {name}', { name: device.address }),
    [h('p', { class: 'dialog-text' }, linkProblem('password').text), field],
    [h('button', { class: 'button', type: 'button', onclick: closeDialog }, t('Cancel')), h('button', { class: 'button primary', type: 'button', onclick: submit }, t('Connect'))],
  );
  field.focus();
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
  const items = [item(t('Rename'), rename)];
  if (device.kind === 'ssh') items.push(item(t('Enter the password'), askPassword));
  else items.push(item(t('Paste a new link'), () => openAddDevice()));
  if (device.phase !== 'live') items.push(item(t('Try again'), () => retryDevice(device.id)));
  items.push(item(t('Forget'), forget));
  openMenu(event.currentTarget, items, false);
}

function onEnter(fields, submit) {
  for (const field of fields) {
    field.addEventListener('keydown', (event) => {
      if (event.key !== 'Enter') return;
      event.preventDefault();
      submit();
    });
  }
}

function linkForm() {
  const field = h('input', { class: 'field', placeholder: LINK_PLACEHOLDER, spellcheck: 'false', 'aria-label': t('Pairing link') });
  return {
    focus: field,
    fields: [field],
    body: [
      h('p', { class: 'dialog-text' }, t('Work on another computer running Octo from this window. End-to-end encrypted.')),
      h('ol', { class: 'pair-steps device-steps' }, ...STEPS.map((step) => h('li', null, step))),
      field,
    ],
    add: () => {
      const pairing = field.value.trim();
      return pairing ? invoke('add_device', { pairing }) : null;
    },
  };
}

function serverForm() {
  const host = h('input', { class: 'field grow', placeholder: t('host or IP'), spellcheck: 'false', 'aria-label': t('Host') });
  const port = h('input', { class: 'field narrow', placeholder: String(SSH_PORT), inputmode: 'numeric', spellcheck: 'false', 'aria-label': t('Port') });
  const user = h('input', { class: 'field grow', placeholder: t('user'), spellcheck: 'false', autocomplete: 'off', 'aria-label': t('User') });
  const password = passwordField();
  password.classList.add('grow');
  return {
    focus: host,
    fields: [host, port, user, password],
    body: [
      h('p', { class: 'dialog-text' }, t('Run agents on a Linux server over SSH, with the agents installed there.')),
      h('div', { class: 'device-fields' }, host, port),
      h('div', { class: 'device-fields' }, user, password),
      h('p', { class: 'dialog-text device-fine' }, t('Leave the password empty to use your SSH key or agent. It stays in memory only and is never saved.')),
      h('p', { class: 'dialog-text device-fine' }, t('On the first connection Octo downloads its server (octo-server, about 8 MB) into ~/.octo/bin on that machine, checks its SHA-256 and runs it as your user. No sudo, nothing outside your home folder.')),
    ],
    add: () => {
      const server = serverFields(host.value, port.value, user.value);
      if (!server) {
        toast(t('Fill in the host and the user.'), 'warn');
        return null;
      }
      const secret = password.value;
      password.value = '';
      return invoke('add_server', { ...server, password: secret || null });
    },
  };
}

export function openAddDevice(kind = 'link') {
  const form = kind === 'ssh' ? serverForm() : linkForm();
  let busy = false;
  const submit = async () => {
    if (busy) return;
    const adding = form.add();
    if (!adding) return;
    busy = true;
    try {
      const device = await adding;
      closeDialog();
      switchDevice(device.id);
    } catch (err) {
      busy = false;
      toast(deviceError(err), 'error');
      form.focus.focus();
    }
  };
  onEnter(form.fields, submit);
  const tab = (name, text) => h('button', { type: 'button', 'aria-pressed': String(kind === name), onclick: () => kind !== name && openAddDevice(name) }, text);
  openDialog(
    t('Add a device'),
    [h('div', { class: 'segmented device-kinds', role: 'group', 'aria-label': t('Add a device') }, tab('link', t('Another Octo')), tab('ssh', t('SSH server'))), ...form.body],
    [
      h('button', { class: 'button', type: 'button', onclick: closeDialog }, t('Cancel')),
      h('button', { class: 'button primary', type: 'button', onclick: submit }, t('Connect')),
    ],
  );
  form.focus.focus();
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
      device && device.kind === 'ssh' ? h('span', { class: 'session-hint' }, 'ssh') : null,
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
    { class: 'session-row new-session', type: 'button', onclick: () => openAddDevice() },
    h('span', { class: 'chevron' }, svg(ICON_PLUS)),
    h('span', { class: 'session-name' }, t('Add a device')),
  );
  place($('devices'), [...all.map((entry) => row(...entry)), add]);
  const rail = $('device-rail');
  rail.hidden = !listing.devices.length;
  place(rail, all.map((entry) => chip(...entry)));
}

function problemActions(problem) {
  const id = activeDevice();
  if (problem.action === 'password') {
    const field = passwordField();
    const send = () => unlock(id, field);
    onEnter([field], send);
    setTimeout(() => field.focus());
    return [field, h('button', { class: 'button primary', type: 'button', onclick: send }, t('Connect'))];
  }
  return [
    problem.action === 'link' ? h('button', { class: 'button primary', type: 'button', onclick: () => openAddDevice() }, t('Paste a new link')) : null,
    problem.action ? h('button', { class: 'button', type: 'button', onclick: () => retryDevice(id) }, t('Try again')) : null,
  ];
}

export function renderLinkProblem(phase, note) {
  const board = $('board');
  const device = currentDevice();
  const problem = linkProblem(phase, device ? device.kind : 'link');
  const signature = `link:${phase}:${note || ''}:${device ? `${device.name}:${device.kind}` : ''}`;
  if (board.dataset.signature === signature) return;
  board.dataset.signature = signature;
  board.replaceChildren(
    h(
      'div',
      { class: 'empty' },
      h('span', { class: 'empty-title' }, device ? `${device.name} · ${problem.title}` : problem.title),
      h('span', null, problem.text),
      note ? h('span', { class: 'empty-note selectable' }, note) : null,
      h('div', { class: 'empty-actions' }, problemActions(problem), h('button', { class: 'button', type: 'button', onclick: () => switchDevice(null) }, t('Back to this computer'))),
    ),
  );
}

export function watchDevices(listed) {
  onListed = listed;
  renderDevices();
  refresh();
  setInterval(refresh, POLL_MS);
}
