import { $, h, svg } from './dom.js';
import { closeDialog, confirm, openDialog } from './dialogs.js';
import { SPINNER } from './format.js';
import { t } from './i18n.js';
import { ICON_PAIR } from './icons.js';
import { state } from './state.js';
import { toast } from './toast.js';
import { invoke } from './transport.js';

const POLL_MS = 1000;
const PHONE_POLL_MS = 3000;
const QUIET_ZONE = 2;
const QR_DARK = '#121218';
const QR_LIGHT = '#ececf0';
const STEPS = [
  t('Point the phone camera at the code'),
  t('Octo opens in the browser, already paired'),
  t('Share › Add to Home Screen keeps it one tap away'),
];
const PRIVACY = t('The tunnel only carries encrypted bytes; the key lives in this code, never on a server.');
const GHOST = Array.from({ length: 21 }, (_, y) => Array.from({ length: 21 }, (_, x) => ((x * 73856093) ^ (y * 19349663)) % 7 < 3 ? '1' : '0').join(''));
const BROWSERS = [
  [/(?:EdgiOS|EdgA|Edg)\/(\d+)/, 'Edge'],
  [/SamsungBrowser\/(\d+)/, 'Samsung Internet'],
  [/(?:OPiOS|OPR)\/(\d+)/, 'Opera'],
  [/(?:FxiOS|Firefox)\/(\d+)/, 'Firefox'],
  [/(?:CriOS|Chrome)\/(\d+)/, 'Chrome'],
  [/Version\/(\d+)[\d.]*.*Safari/, 'Safari'],
];

let timer = null;
let shown = '';

export function qrPath(rows) {
  const parts = [];
  rows.forEach((row, y) => {
    for (let x = 0; x < row.length; x += 1) {
      if (row[x] !== '1') continue;
      let end = x;
      while (row[end + 1] === '1') end += 1;
      parts.push(`M${x} ${y}h${end - x + 1}v1h-${end - x + 1}z`);
      x = end;
    }
  });
  return parts.join('');
}

export function phoneConnected(pairing) {
  return Boolean(pairing.enabled) && pairing.phase === 'ready' && pairing.phones > 0;
}

export function phoneStatus(pairing) {
  if (!pairing.enabled) return 'off';
  if (pairing.phase === 'failed') return 'failed';
  if (pairing.phase !== 'ready') return 'pending';
  return pairing.phones > 0 ? 'connected' : 'ready';
}

export function describeDevice(device) {
  const agent = device.agent || '';
  const found = BROWSERS.find(([pattern]) => pattern.test(agent));
  const browser = found ? `${found[1]} ${found[0].exec(agent)[1]}` : null;
  const apple = /\b(iPhone|iPad|iPod)\b/.exec(agent)?.[1] || (device.model === 'iPad' ? 'iPad' : null);
  if (apple) {
    const version = /(?:iPhone|CPU) OS (\d+)_(\d+)/.exec(agent);
    const tablet = apple === 'iPad';
    const number = version ? ` ${version[1]}${version[2] === '0' ? '' : `.${version[2]}`}` : '';
    return { name: apple, os: `${tablet ? 'iPadOS' : 'iOS'}${number}`, browser: browser || 'Safari' };
  }
  const android = /Android(?: (\d+))?/.exec(agent);
  if (android) {
    const tablet = !/Mobile/.test(agent);
    const version = device.osVersion?.split('.')[0] || (/; K\)/.test(agent) ? null : android[1]);
    return { name: device.model || (tablet ? t('Android tablet') : t('Android phone')), os: version ? `Android ${version}` : 'Android', browser };
  }
  const desktop = [[/Windows/, 'Windows'], [/CrOS/, 'ChromeOS'], [/Mac OS X|Macintosh/, 'macOS'], [/Linux/, 'Linux']].find(([pattern]) => pattern.test(agent));
  return { name: desktop ? t('{os} computer', { os: desktop[1] }) : t('Unknown device'), os: desktop?.[1] || null, browser };
}

export function connectedFor(sinceMs, now = Date.now()) {
  const minutes = Math.floor(Math.max(0, now - sinceMs) / 60000);
  if (minutes < 1) return t('just now');
  if (minutes < 60) return t('{n} min', { n: minutes });
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return t('{h} h {m} min', { h: hours, m: String(minutes % 60).padStart(2, '0') });
  return t('{d} d {h} h', { d: Math.floor(hours / 24), h: hours % 24 });
}

export function renderPhoneStatus(pairing) {
  const button = $('pair');
  const status = phoneStatus(pairing);
  const named = status === 'connected' && pairing.devices?.length === 1 && pairing.devices[0].agent ? describeDevice(pairing.devices[0]).name : null;
  const key = `${status}:${pairing.phones}:${named || ''}`;
  if (button.dataset.key === key) return;
  button.dataset.key = key;
  button.dataset.state = status;
  const title = named || (status === 'connected' ? t('{count} connected', { count: pairing.phones }) : t('Pair phone'));
  const detail = { off: t('Phone access is off'), pending: t('Starting…'), failed: t('Pairing needs attention'), ready: t('Waiting for a phone'), connected: t('Live · end-to-end encrypted') }[status];
  button.setAttribute('aria-label', `${title} · ${detail}`);
  button.title = `${title} · ${detail} (ctrl P)`;
}

export function waiting(pairing) {
  if (!pairing.phase) return { text: t('Starting Octo for the phone…'), tone: 't-yellow' };
  if (pairing.phase === 'installing') return { text: t('Downloading cloudflared, the official build from github.com/cloudflare…'), tone: 't-yellow' };
  if (pairing.phase === 'connecting') return { text: t('Opening the {tunnel} tunnel…', { tunnel: pairing.tunnel }), tone: 't-yellow' };
  if (pairing.phase === 'failed') return { text: `${pairing.error || t('The tunnel failed')} · ${t('retrying')}`, tone: 't-red' };
  return { text: t('The pairing code is not ready'), tone: 't-muted' };
}

function code(rows) {
  const size = rows.length + QUIET_ZONE * 2;
  return svg(
    `<svg class="pair-qr" viewBox="-${QUIET_ZONE} -${QUIET_ZONE} ${size} ${size}" shape-rendering="crispEdges" role="img" aria-label="${t('Pairing code')}"><rect x="-${QUIET_ZONE}" y="-${QUIET_ZONE}" width="${size}" height="${size}" fill="${QR_LIGHT}"/><path d="${qrPath(rows)}" fill="${QR_DARK}"/></svg>`,
  );
}

function copy(link) {
  navigator.clipboard
    .writeText(link)
    .then(() => toast(t("Copied the pairing link · paste it in the phone's browser"), 'success'))
    .catch(() => toast(t('Could not copy the link.'), 'warn'));
}

function detail(label, ...value) {
  return h('div', { class: 'pair-detail' }, h('span', { class: 'pair-label' }, label), h('span', { class: 'pair-value' }, ...value));
}

function statePanel(pairing) {
  const pending = pairing.enabled && pairing.phase !== 'failed';
  const message = pairing.enabled ? waiting(pairing).text : t('Turn on to get a pairing code');
  return h('div', { class: 'pair-state' },
    h('div', { class: 'pair-placeholder', 'aria-hidden': 'true' }, svg(`<svg class="pair-ghost" viewBox="0 0 21 21" shape-rendering="crispEdges"><path d="${qrPath(GHOST)}" fill="currentColor"/></svg>`),
      h('span', { class: 'pair-placeholder-mark' }, pending ? SPINNER[state.frame % SPINNER.length] : svg(ICON_PAIR))),
    h('div', { class: 'pair-state-message', role: 'status' }, h('strong', null, pairing.enabled ? t(pairing.phase === 'failed' ? 'Pairing needs attention' : 'Preparing a secure connection') : t('Phone access is off')), h('span', null, message)));
}

function deviceCard(device) {
  const about = device.agent ? describeDevice(device) : { name: t('Phone'), os: null, browser: null };
  const meta = device.agent ? [about.os, about.browser, device.standalone ? t('Home Screen app') : t('in the browser')].filter(Boolean).join(' · ') : t('Live · end-to-end encrypted');
  return h('li', { class: 'pair-device' },
    h('span', { class: 'pair-device-name' }, about.name),
    h('span', { class: 'pair-device-meta', title: device.agent || '' }, meta),
    device.sinceMs ? h('span', { class: 'pair-device-since', dataset: { since: device.sinceMs } }, connectedFor(device.sinceMs)) : null);
}

function body(pairing) {
  if (!pairing.enabled) {
    return statePanel(pairing);
  }
  if (!pairing.link || !pairing.qr) {
    return statePanel(pairing);
  }
  const phones = pairing.phones === 0 ? h('span', { class: 't-faint' }, t('no phone connected')) : h('span', { class: 't-green' }, t('{count} connected', { count: pairing.phones }));
  return h(
    'div',
    { class: 'pair' },
    code(pairing.qr),
    h(
      'div',
      { class: 'pair-side' },
      h('ol', { class: 'pair-steps' }, ...STEPS.map((step) => h('li', null, step))),
      detail(t('tunnel'), `${pairing.tunnel} · ${t('ready')}`),
      detail(t('link'), h('span', { class: 'pair-link', title: pairing.link }, pairing.link), h('button', { class: 'button small', type: 'button', onclick: () => copy(pairing.link) }, t('Copy'))),
      detail(t('phones'), phones),
      pairing.phones > 0 ? h('ul', { class: 'pair-devices', 'aria-label': t('Connected devices') }, ...(pairing.devices?.length ? pairing.devices : Array.from({ length: pairing.phones }, () => ({}))).map(deviceCard)) : null,
      detail(t('privacy'), h('span', { class: 't-faint' }, PRIVACY)),
    ),
  );
}

async function act(command, args) {
  try {
    render(await invoke(command, args));
  } catch (err) {
    toast(String(err), 'error');
  }
}

async function renewCode() {
  stop();
  const sure = await confirm({
    title: t('Make a new pairing code?'),
    text: t('The phones paired with the current code are forgotten and have to scan the new one.'),
    action: t('New code'),
    danger: true,
  });
  await openPairing();
  if (sure) {
    toast(t('Forgetting the paired phones…'), 'info');
    await act('renew_phone');
  }
}

function actions(pairing) {
  const close = h('button', { class: 'button', type: 'button', onclick: closeDialog }, t('Close'));
  if (!pairing.enabled) return [close, h('button', { class: 'button primary', type: 'button', onclick: () => act('set_phone', { on: true }) }, t('Turn on'))];
  return [
    h('button', { class: 'button', type: 'button', onclick: renewCode }, t('New code')),
    h('button', { class: 'button warn', type: 'button', onclick: () => act('set_phone', { on: false }) }, t('Turn off')),
    close,
  ];
}

function markPhone(pairing) {
  $('pair').classList.toggle('connected', phoneConnected(pairing));
  renderPhoneStatus(pairing);
}

function render(pairing) {
  markPhone(pairing);
  const signature = JSON.stringify(pairing);
  if (signature === shown && $('dialog').open) return;
  shown = signature;
  const title = [h('span', null, t('Pair your phone')), statusPill(pairing)];
  const tagline = h('p', { class: 'pair-tagline' }, t('Octo on your phone, from anywhere · end-to-end encrypted'));
  const dialog = openDialog(title, [tagline, body(pairing)], actions(pairing));
  dialog.classList.add('pairing');
}

function statusPill(pairing) {
  const status = phoneStatus(pairing);
  const text = { off: t('Off'), pending: t('Starting…'), failed: t('Pairing needs attention'), ready: t('Waiting for a phone'), connected: t('Live') }[status];
  return h('span', { class: `pair-badge pair-badge-${status}` }, text);
}

function stop() {
  clearInterval(timer);
  timer = null;
  shown = '';
  $('dialog').classList.remove('pairing');
}

export function watchPhone() {
  const check = () => invoke('pairing').then(markPhone).catch(() => {});
  check();
  setInterval(check, PHONE_POLL_MS);
}

export async function openPairing() {
  try {
    render(await invoke('pairing'));
  } catch (err) {
    toast(String(err), 'error');
    return;
  }
  if (timer) return;
  timer = setInterval(async () => {
    if (!$('dialog').open || !$('dialog').classList.contains('pairing')) {
      stop();
      return;
    }
    try {
      render(await invoke('pairing'));
      for (const node of document.querySelectorAll('.pairing [data-since]')) node.textContent = connectedFor(Number(node.dataset.since));
    } catch (err) { closeDialog(); toast(String(err), 'error'); }
  }, POLL_MS);
  $('dialog').addEventListener('close', stop, { once: true });
}
