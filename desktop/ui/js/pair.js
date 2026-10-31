import { $, h, svg } from './dom.js';
import { closeDialog, confirm, openDialog } from './dialogs.js';
import { SPINNER } from './format.js';
import { state } from './state.js';
import { toast } from './toast.js';
import { invoke } from './transport.js';

const POLL_MS = 1000;
const QUIET_ZONE = 2;
const QR_DARK = '#121218';
const QR_LIGHT = '#ececf0';
const STEPS = [
  'Point the phone camera at the code',
  'Your orkai opens in the browser, already paired',
  'Share › Add to Home Screen keeps it one tap away',
];
const PRIVACY = 'The tunnel only carries encrypted bytes; the key lives in this code, never on a server.';

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

export function badge(pairing) {
  if (!pairing.enabled) return { text: 'off', tone: 't-faint' };
  if (pairing.phase === 'failed') return { text: 'error', tone: 't-red' };
  if (pairing.phase === 'ready' && pairing.phones > 0) return { text: `● ${pairing.phones}`, tone: 't-green' };
  if (pairing.phase === 'ready') return { text: 'on', tone: 't-green' };
  return { text: '…', tone: 't-yellow' };
}

export function waiting(pairing) {
  if (!pairing.phase) return { text: 'Starting the orkai pwa…', tone: 't-yellow' };
  if (pairing.phase === 'installing') return { text: 'Downloading cloudflared, the official build from github.com/cloudflare…', tone: 't-yellow' };
  if (pairing.phase === 'connecting') return { text: `Opening the ${pairing.tunnel} tunnel…`, tone: 't-yellow' };
  if (pairing.phase === 'failed') return { text: `${pairing.error || 'The tunnel failed'} · retrying`, tone: 't-red' };
  return { text: 'The pairing code is not ready', tone: 't-muted' };
}

function code(rows) {
  const size = rows.length + QUIET_ZONE * 2;
  return svg(
    `<svg class="pair-qr" viewBox="-${QUIET_ZONE} -${QUIET_ZONE} ${size} ${size}" shape-rendering="crispEdges" role="img" aria-label="Pairing code"><rect x="-${QUIET_ZONE}" y="-${QUIET_ZONE}" width="${size}" height="${size}" fill="${QR_LIGHT}"/><path d="${qrPath(rows)}" fill="${QR_DARK}"/></svg>`,
  );
}

function copy(link) {
  navigator.clipboard
    .writeText(link)
    .then(() => toast("Copied the pairing link · paste it in the phone's browser", 'success'))
    .catch(() => toast('Could not copy the link.', 'warn'));
}

function detail(label, ...value) {
  return h('div', { class: 'pair-detail' }, h('span', { class: 'pair-label' }, label), h('span', { class: 'pair-value' }, ...value));
}

function body(pairing) {
  if (!pairing.enabled) {
    return h('p', { class: 'dialog-text' }, 'The orkai pwa is off. Turn it on to show the pairing code: your agents on the phone, from anywhere.');
  }
  if (!pairing.link || !pairing.qr) {
    const text = waiting(pairing);
    return h('div', { class: `pair-waiting ${text.tone}` }, h('span', { class: 'glyph', dataset: { spin: '1' } }, SPINNER[state.frame % SPINNER.length]), text.text);
  }
  const phones = pairing.phones === 0 ? h('span', { class: 't-faint' }, 'no phone connected') : h('span', { class: 't-green' }, `${pairing.phones} connected`);
  return h(
    'div',
    { class: 'pair' },
    code(pairing.qr),
    h(
      'div',
      { class: 'pair-side' },
      h('ol', { class: 'pair-steps' }, ...STEPS.map((step) => h('li', null, step))),
      detail('tunnel', `${pairing.tunnel} · ready`),
      detail('link', h('span', { class: 'pair-link', title: pairing.link }, pairing.link), h('button', { class: 'button small', type: 'button', onclick: () => copy(pairing.link) }, 'Copy')),
      detail('phones', phones),
      detail('privacy', h('span', { class: 't-faint' }, PRIVACY)),
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
    title: 'Make a new pairing code?',
    text: 'The phones paired with the current code are forgotten and have to scan the new one.',
    action: 'New code',
    danger: true,
  });
  await openPairing();
  if (sure) {
    toast('Forgetting the paired phones…', 'info');
    await act('renew_phone');
  }
}

function actions(pairing) {
  const close = h('button', { class: 'button', type: 'button', onclick: closeDialog }, 'Close');
  if (!pairing.enabled) return [close, h('button', { class: 'button primary', type: 'button', onclick: () => act('set_phone', { on: true }) }, 'Turn on')];
  return [
    h('button', { class: 'button', type: 'button', onclick: renewCode }, 'New code'),
    h('button', { class: 'button warn', type: 'button', onclick: () => act('set_phone', { on: false }) }, 'Turn off'),
    close,
  ];
}

function render(pairing) {
  const signature = JSON.stringify(pairing);
  if (signature === shown && $('dialog').open) return;
  shown = signature;
  const mark = badge(pairing);
  const title = [h('span', null, 'Pair your phone'), h('span', { class: `pair-badge ${mark.tone}` }, mark.text)];
  const tagline = h('p', { class: 'pair-tagline' }, 'Your orkai on the phone, from anywhere · end-to-end encrypted');
  const dialog = openDialog(title, [tagline, body(pairing)], actions(pairing));
  dialog.classList.add('pairing');
}

function stop() {
  clearInterval(timer);
  timer = null;
  shown = '';
  $('dialog').classList.remove('pairing');
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
    render(await invoke('pairing'));
  }, POLL_MS);
  $('dialog').addEventListener('close', stop, { once: true });
}
