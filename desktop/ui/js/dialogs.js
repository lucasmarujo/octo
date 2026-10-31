import { $, h } from './dom.js';
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
