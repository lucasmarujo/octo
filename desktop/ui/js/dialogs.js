import { $, h } from './dom.js';
import { sgrRuns } from './palette.js';
import { send } from './transport.js';

let folderPick = null;

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
  folderPick = null;
  if (dialog.open) dialog.close();
}

const MENU_GAP = 6;
const MENU_MARGIN = 8;

export function closeMenu() {
  $('menu').hidden = true;
}

export function openMenu(anchor, items, above) {
  const menu = $('menu');
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
      h('button', { class: 'button', type: 'button', onclick: cancel }, 'Cancel'),
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
      h('button', { class: 'button', type: 'button', onclick: cancel }, 'Cancel'),
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
  if (message.truncated || lines.length > DIFF_LINES) notes.push(h('span', { class: 'detail-note' }, 'The diff is too long to show whole; the rest is on the branch.'));
  const body = message.text.trim()
    ? h('pre', { class: 'diff selectable' }, shown)
    : h('p', { class: 'dialog-text' }, 'No changes on this branch yet.');
  dialog.classList.add('wide');
  open(`Changes on ${message.branch}`, [body, notes], [h('button', { class: 'button primary', type: 'button', onclick: close }, 'Close')]);
  dialog.addEventListener('close', () => dialog.classList.remove('wide'), { once: true });
}

export function showTests(message) {
  const dialog = $('dialog');
  const lines = message.text.split('\n');
  const style = { color: null, bold: false, dim: false };
  const shown = lines.slice(-DIFF_LINES).map((line) => diffLine(line, style));
  const notes = message.truncated ? [h('span', { class: 'detail-note' }, 'Only the end of the output is shown.')] : [];
  const body = message.text.trim() ? h('pre', { class: 'diff selectable' }, shown) : h('p', { class: 'dialog-text' }, 'The test printed nothing.');
  dialog.classList.add('wide');
  open(`${message.command} · ${message.state}`, [body, notes], [h('button', { class: 'button primary', type: 'button', onclick: close }, 'Close')]);
  dialog.addEventListener('close', () => dialog.classList.remove('wide'), { once: true });
}

export function pickFolder(start, onPick) {
  const dialog = $('dialog');
  folderPick = { onPick, path: start };
  const list = h('div', { class: 'folders', id: 'folder-list' }, h('div', { class: 'folder up' }, 'Loading…'));
  const path = h('span', { class: 'folder-path', id: 'folder-path' }, start);
  open('Where should the agents run?', [path, list], [
    h('button', { class: 'button', type: 'button', onclick: close }, 'Cancel'),
    h('button', {
      class: 'button primary',
      type: 'button',
      onclick: () => {
        const pick = folderPick;
        close();
        if (pick) pick.onPick(pick.path);
      },
    }, 'Use this folder'),
  ]);
  dialog.addEventListener('close', () => {
    folderPick = null;
  }, { once: true });
  send({ type: 'folders', path: start });
}

export function showFolders(message) {
  if (!folderPick) return;
  folderPick.path = message.path;
  const list = $('folder-list');
  const path = $('folder-path');
  if (!list || !path) return;
  path.textContent = message.path;
  const go = (target) => () => send({ type: 'folders', path: target });
  const rows = [];
  if (message.parent) rows.push(h('button', { class: 'folder up', type: 'button', onclick: go(message.parent) }, '..'));
  for (const dir of message.dirs) rows.push(h('button', { class: 'folder', type: 'button', onclick: go(dir.path) }, `${dir.name}/`));
  if (!message.dirs.length) rows.push(h('div', { class: 'folder up' }, 'No folders inside'));
  list.replaceChildren(...rows);
}

export { close as closeDialog, open as openDialog };
