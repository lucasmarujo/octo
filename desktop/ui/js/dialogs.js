import { $, h } from './dom.js';
import { t } from './i18n.js';
import { motionMs } from './motion.js';
import { sgrRuns } from './palette.js';

let menuClosing = null;
let menuOnClose = null;

function open(title, body, actions) {
  const dialog = $('dialog');
  dialog.replaceChildren(
    h('div', { class: 'dialog-body' }, h('span', { class: 'dialog-title' }, title), body),
    h('div', { class: 'dialog-actions' }, actions),
  );
  if (!dialog.open) dialog.showModal();
  return dialog;
}

function close() {
  const dialog = $('dialog');
  if (dialog.open) dialog.close();
}

const MENU_GAP = 6;
const MENU_MARGIN = 8;

export function menuShown() {
  return !$('menu').hidden && menuClosing === null;
}

function menuEnded() {
  const onClose = menuOnClose;
  menuOnClose = null;
  if (onClose) onClose();
}

export function closeMenu() {
  const menu = $('menu');
  if (menu.hidden || menuClosing !== null) return;
  menuEnded();
  menu.classList.add('closing');
  menuClosing = setTimeout(() => {
    menuClosing = null;
    menu.classList.remove('closing');
    menu.hidden = true;
  }, motionMs('--motion-fast'));
}

export function openMenu(anchor, items, above, onClose = null) {
  const menu = $('menu');
  menuEnded();
  menuOnClose = onClose;
  clearTimeout(menuClosing);
  menuClosing = null;
  menu.classList.remove('closing');
  menu.dataset.side = above ? 'above' : 'below';
  menu.replaceChildren(...items);
  menu.hidden = false;
  const box = anchor.getBoundingClientRect();
  const left = Math.min(box.left, window.innerWidth - menu.offsetWidth - MENU_MARGIN);
  menu.style.left = `${Math.max(MENU_MARGIN, left)}px`;
  menu.style.top = above ? `${Math.max(MENU_MARGIN, box.top - menu.offsetHeight - MENU_GAP)}px` : `${box.bottom + MENU_GAP}px`;
}

export function confirm({ title, text, action, danger }) {
  return new Promise((resolve) => {
    const dialog = $('dialog');
    const done = (answer) => {
      dialog.removeEventListener('close', cancel);
      close();
      resolve(answer);
    };
    const cancel = () => done(false);
    const yes = h('button', { class: danger ? 'button warn' : 'button primary', type: 'button', onclick: () => done(true) }, action);
    open(title, h('p', { class: 'dialog-text' }, text), [
      h('button', { class: 'button', type: 'button', onclick: cancel }, t('Cancel')),
      yes,
    ]);
    dialog.addEventListener('close', cancel, { once: true });
    yes.focus();
  });
}

export function ask({ title, text, value, action, placeholder }) {
  return new Promise((resolve) => {
    const dialog = $('dialog');
    const field = h('input', { class: 'field', value: value || '', placeholder: placeholder || '', spellcheck: 'false' });
    const done = (answer) => {
      dialog.removeEventListener('close', cancel);
      close();
      resolve(answer);
    };
    const cancel = () => done(null);
    const submit = () => {
      const answer = field.value.trim();
      if (answer) done(answer);
    };
    field.addEventListener('keydown', (event) => {
      if (event.key === 'Enter') {
        event.preventDefault();
        submit();
      }
    });
    open(title, [text ? h('p', { class: 'dialog-text' }, text) : null, field], [
      h('button', { class: 'button', type: 'button', onclick: cancel }, t('Cancel')),
      h('button', { class: 'button primary', type: 'button', onclick: submit }, action),
    ]);
    dialog.addEventListener('close', cancel, { once: true });
    field.focus();
    field.select();
  });
}

const DIFF_LINES = 4000;

function diffLine(line, style) {
  const runs = sgrRuns(line, style);
  const spans = runs.map((run) => {
    const css = [run.color ? `color:${run.color}` : '', run.bold ? 'font-weight:700' : '', run.dim ? 'opacity:0.6' : ''].filter(Boolean).join(';');
    return css ? h('span', { style: css }, run.text) : run.text;
  });
  return h('div', { class: 'diff-line' }, spans.length ? spans : ' ');
}

export function showDiff(message) {
  const dialog = $('dialog');
  const lines = message.text.split('\n');
  const style = { color: null, bold: false, dim: false };
  const shown = lines.slice(0, DIFF_LINES).map((line) => diffLine(line, style));
  const notes = [];
  if (message.truncated || lines.length > DIFF_LINES) notes.push(h('span', { class: 'detail-note' }, t('The diff is too long to show whole; the rest is on the branch.')));
  const body = message.text.trim()
    ? h('pre', { class: 'diff selectable' }, shown)
    : h('p', { class: 'dialog-text' }, t('No changes on this branch yet.'));
  dialog.classList.add('wide');
  open(t('Changes on {branch}', { branch: message.branch }), [body, notes], [h('button', { class: 'button primary', type: 'button', onclick: close }, t('Close'))]);
  dialog.addEventListener('close', () => dialog.classList.remove('wide'), { once: true });
}

function report(title, text, empty, notes) {
  const dialog = $('dialog');
  const style = { color: null, bold: false, dim: false };
  const shown = text.split('\n').slice(-DIFF_LINES).map((line) => diffLine(line, style));
  const body = text.trim() ? h('pre', { class: 'diff selectable' }, shown) : h('p', { class: 'dialog-text' }, empty);
  dialog.classList.add('wide');
  open(title, [body, notes], [h('button', { class: 'button primary', type: 'button', onclick: close }, t('Close'))]);
  dialog.addEventListener('close', () => dialog.classList.remove('wide'), { once: true });
}

export function showTests(message) {
  const notes = message.truncated ? [h('span', { class: 'detail-note' }, t('Only the end of the output is shown.'))] : [];
  report(`${message.command} · ${t(message.state)}`, message.text, t('The test printed nothing.'), notes);
}

export function showCompare(message) {
  report(t('Race · {title}', { title: message.title }), message.text, t('No agent of the race changed anything yet.'), []);
}

export { close as closeDialog, open as openDialog };
