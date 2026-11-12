import { $, h, svg } from './dom.js';
import { acceptedSuggestion, matchingCommands, nextSuggestion, parseCommand } from './commands.js';
import { closeMenu, menuShown, openMenu } from './dialogs.js';
import { pickFolder } from './folders.js';
import { MAX_ATTACHMENTS, fileName, kindColor, label, optionsTone, withAttachments } from './format.js';
import { t } from './i18n.js';
import { ICON_ATTACH, ICON_BRANCH, ICON_CHEVRON_DOWN, ICON_OPTIONS, ICON_REMOTE, ICON_SHIELD } from './icons.js';
import { flash } from './motion.js';
import {
  currentModel,
  currentWhere,
  currentWho,
  installedKinds,
  modelsOf,
  setBypass,
  setModel,
  setRemote,
  setWhere,
  setWho,
  setWorktree,
  state,
  worktreeOn,
} from './state.js';
import { toast } from './toast.js';
import { invoke, onFileDrop, send } from './transport.js';

const MAX_HEIGHT = 160;
const MAX_SUGGESTIONS = 8;
const SKILLS_TTL_MS = 5000;
const MODELS_TTL_MS = 60_000;
const TASK_OPTIONS = [
  { name: t('Worktree'), icon: ICON_BRANCH, tone: 'var(--color-link)', title: t('Each new AI agent runs in its own git worktree and octo/<name> branch'), on: worktreeOn, set: setWorktree },
  { name: t('Remote Control'), icon: ICON_REMOTE, tone: 'var(--color-waiting)', title: t('New Claude agents start with Remote Control: continue them from claude.ai or the Claude app'), on: () => state.remote, set: setRemote },
  { name: t('Bypass'), icon: ICON_SHIELD, tone: 'var(--color-lost)', title: t('New AI agents run tools, commands and edits without asking. Only in folders you trust.'), on: () => state.bypass, set: setBypass },
];

let runCommand = () => false;
let paletteItems = [];
let paletteIndex = 0;
let paletteWord = null;
let optionRows = null;

export function setCommandRunner(run) {
  runCommand = run;
}

function skillNames() {
  const skills = state.skills;
  return skills && skills.kind === currentWho() && skills.cwd === currentWhere() ? skills.names : [];
}

function askSkills() {
  const skills = state.skills;
  const fresh = skills && skills.kind === currentWho() && skills.cwd === currentWhere() && Date.now() - skills.at < SKILLS_TTL_MS;
  if (fresh || currentWho() === 'terminal') return;
  state.skills = { kind: currentWho(), cwd: currentWhere(), names: skills && skills.kind === currentWho() ? skills.names : [], at: Date.now() };
  send({ type: 'skills', kind: currentWho(), cwd: currentWhere() });
}

function askModels() {
  const kind = currentWho();
  const known = state.models[kind];
  if (kind === 'terminal' || (known && Date.now() - known.at < MODELS_TTL_MS)) return;
  state.models[kind] = { list: known ? known.list : [], at: Date.now() };
  send({ type: 'models', kind });
}

function typedWord() {
  const text = $('task').value;
  if (!text.startsWith('/') || text.includes('\n')) return null;
  const space = text.search(/\s/);
  return space === -1 ? text : null;
}

export function renderPalette() {
  const palette = $('palette');
  const word = typedWord();
  if (word !== paletteWord) {
    paletteWord = word;
    paletteIndex = 0;
  }
  if (word === null) {
    paletteItems = [];
    palette.hidden = true;
    return;
  }
  askSkills();
  const commands = matchingCommands(word).map(([usage, about]) => ({ text: usage, about, kind: 'command' }));
  const skills = skillNames()
    .filter((skill) => `/${skill}`.startsWith(word))
    .map((skill) => ({ text: `/${skill}`, about: `skill · ${label(currentWho())}`, kind: 'skill', color: kindColor(currentWho()) }));
  paletteItems = [...commands, ...skills].slice(0, MAX_SUGGESTIONS);
  paletteIndex = Math.min(paletteIndex, Math.max(paletteItems.length - 1, 0));
  palette.hidden = paletteItems.length === 0;
  palette.replaceChildren(
    ...paletteItems.map((item, index) =>
      h(
        'button',
        {
          class: `palette-item ${item.kind}${index === paletteIndex ? ' active' : ''}`,
          type: 'button',
          role: 'option',
          'aria-selected': String(index === paletteIndex),
          onmousedown: (event) => event.preventDefault(),
          onclick: () => acceptSuggestion(item),
        },
        h('span', { class: 'palette-usage', style: item.color && `color:${item.color}` }, item.text),
        h('span', { class: 'palette-about' }, item.about),
      ),
    ),
  );
}

function acceptSuggestion(item) {
  $('task').value = acceptedSuggestion(item.text, item.kind);
  $('task').focus();
  renderPalette();
}

function completeTyped() {
  if (typedWord() === null) return false;
  if (paletteItems.length) acceptSuggestion(paletteItems[paletteIndex]);
  return true;
}

function moveSuggestion(step) {
  paletteIndex = nextSuggestion(paletteIndex, step, paletteItems.length);
  renderPalette();
}

function grow() {
  const task = $('task');
  task.style.height = 'auto';
  const height = Math.min(task.scrollHeight, MAX_HEIGHT);
  task.style.height = `${height}px`;
  task.style.overflowY = task.scrollHeight > MAX_HEIGHT ? 'auto' : 'hidden';
}

function openWhoMenu() {
  const current = currentWho();
  const items = installedKinds().map((kind) =>
    h(
      'button',
      {
        class: `menu-item${kind === current ? ' active' : ''}`,
        type: 'button',
        role: 'menuitem',
        onclick: () => {
          setWho(kind);
          closeMenu();
          renderComposer();
          renderPalette();
          $('task').focus();
        },
      },
      h('span', { style: `color:${kindColor(kind)}` }, label(kind)),
      kind === current ? h('span', { class: 'check' }, '✓') : null,
    ),
  );
  openMenu($('who'), items, true);
}

function modelItem(kind, model, current) {
  const active = (model ? model.id : null) === (current ? current.id : null);
  return h(
    'button',
    {
      class: `menu-item${active ? ' active' : ''}`,
      type: 'button',
      role: 'menuitem',
      title: model ? model.description || model.id : t('Whatever the agent is set to use'),
      onclick: () => {
        setModel(kind, model ? model.id : null);
        closeMenu();
        renderComposer();
        $('task').focus();
      },
    },
    h('span', null, model ? model.name : t('Default model')),
    active ? h('span', { class: 'check' }, '✓') : null,
  );
}

function openModelMenu() {
  const kind = currentWho();
  const current = currentModel();
  const items = [modelItem(kind, null, current), ...modelsOf(kind).map((model) => modelItem(kind, model, current))];
  openMenu($('model'), items, true);
}

function renderModel() {
  askModels();
  const kind = currentWho();
  const button = $('model');
  button.hidden = kind === 'terminal' || modelsOf(kind).length === 0;
  const current = currentModel();
  const name = current ? current.name : t('Default model');
  button.replaceChildren(h('span', { class: 'value' }, name), svg(ICON_CHEVRON_DOWN));
  button.setAttribute('aria-label', `${t('Model the agent starts with')}: ${name}`);
  button.title = t('Model the agent starts with');
}

export function toggleWorktree() {
  setWorktree(!worktreeOn());
  renderComposer();
}

function flipOption(option) {
  option.set(!option.on());
  renderComposer();
}

function optionRow(option) {
  return h(
    'button',
    {
      class: 'menu-item option-item',
      type: 'button',
      role: 'menuitemcheckbox',
      title: option.title,
      style: `--tone:${option.tone}`,
      onclick: () => flipOption(option),
    },
    svg(option.icon),
    h('span', null, option.name),
    h('span', { class: 'switch', 'aria-hidden': 'true' }, h('span', { class: 'switch-knob' })),
  );
}

function renderOptionRows() {
  if (!optionRows) return;
  optionRows.forEach((row, index) => {
    const on = String(TASK_OPTIONS[index].on());
    row.setAttribute('aria-checked', on);
    row.querySelector('.switch').setAttribute('aria-checked', on);
  });
}

function optionsClosed() {
  optionRows = null;
  $('options').setAttribute('aria-expanded', 'false');
  const focused = document.activeElement;
  if (focused === document.body || focused === $('options') || $('menu').contains(focused)) $('task').focus();
}

function openOptionsMenu(event) {
  optionRows = TASK_OPTIONS.map(optionRow);
  renderOptionRows();
  openMenu($('options'), optionRows, true, optionsClosed);
  $('options').setAttribute('aria-expanded', 'true');
  if (event.detail === 0) optionRows[0].focus();
}

function renderOptionsButton() {
  const tone = optionsTone({ worktree: worktreeOn(), remote: state.remote, bypass: state.bypass });
  const active = TASK_OPTIONS.filter((option) => option.on()).map((option) => option.name);
  const name = active.length ? `${t('Task options')} · ${active.join(', ')}` : t('Task options');
  const button = $('options');
  button.classList.toggle('caution', tone === 'caution');
  button.classList.toggle('danger', tone === 'danger');
  button.setAttribute('aria-pressed', String(tone !== 'neutral'));
  button.title = name;
  button.setAttribute('aria-label', name);
}

function attach(paths) {
  const wanted = new Set([...state.attachments, ...paths]).size;
  state.attachments = withAttachments(state.attachments, paths);
  if (wanted > MAX_ATTACHMENTS) toast(t('A task takes up to {max} attached files.', { max: MAX_ATTACHMENTS }), 'warn');
  renderAttachments();
}

function detach(path) {
  state.attachments = state.attachments.filter((file) => file !== path);
  renderAttachments();
  $('task').focus();
}

function chooseFiles() {
  invoke('pick_files')
    .then((paths) => {
      attach(paths);
      $('task').focus();
    })
    .catch((error) => toast(t('could not choose the files: {error}', { error: String(error) }), 'error'));
}

function takesFiles() {
  return currentWho() !== 'terminal' && $('composer').offsetParent !== null;
}

function onDrop(drop) {
  const over = (drop.type === 'enter' || drop.type === 'over') && takesFiles();
  $('composer').classList.toggle('dropping', over);
  if (drop.type === 'drop' && takesFiles()) attach(drop.paths);
}

function renderAttachments() {
  const ai = currentWho() !== 'terminal';
  const files = state.attachments;
  const name = files.length ? t('Attach files · {count} attached', { count: files.length }) : t('Attach files');
  const button = $('attach');
  button.hidden = !ai;
  button.setAttribute('aria-pressed', String(files.length > 0));
  button.setAttribute('aria-label', name);
  button.title = files.length ? name : t('Attach files to the task, or drop them here');
  $('attachments').hidden = !ai || files.length === 0;
  $('attachments').replaceChildren(
    ...files.map((path) =>
      h(
        'span',
        { class: 'attachment', title: path },
        h('span', { class: 'attachment-name' }, fileName(path)),
        h('button', { class: 'attachment-remove', type: 'button', 'aria-label': t('Remove {name}', { name: fileName(path) }), onclick: () => detach(path) }, '×'),
      ),
    ),
  );
}

function cycleWho(step) {
  const kinds = installedKinds();
  if (!kinds.length) return;
  const at = kinds.indexOf(currentWho());
  setWho(kinds[(at + step + kinds.length) % kinds.length]);
  renderComposer();
  renderPalette();
}

function recall(step) {
  if (!state.history.length) return;
  const next = state.recall === -1 ? (step < 0 ? state.history.length - 1 : -1) : state.recall + step;
  if (next < 0 || next >= state.history.length) {
    state.recall = -1;
    $('task').value = '';
  } else {
    state.recall = next;
    $('task').value = state.history[next];
  }
  grow();
}

function submit(event) {
  event.preventDefault();
  if (!state.overview || !state.session) return;
  const task = $('task');
  const raw = task.value;
  const text = raw.trim();
  if (parseCommand(text) && runCommand(text)) {
    if (text) state.history.push(raw);
    state.recall = -1;
    task.value = '';
    grow();
    renderPalette();
    return;
  }
  let kind = currentWho();
  let command = raw;
  if (text.startsWith('!')) {
    kind = 'terminal';
    command = text.slice(1).trim();
  } else if (kind !== 'terminal' && !text && !state.attachments.length) {
    toast(t('Write the task first.'), 'warn');
    return;
  }
  const ai = kind !== 'terminal';
  send({
    type: 'start',
    kind,
    session: state.session,
    cwd: currentWhere(),
    task: ai ? raw : command,
    worktree: worktreeOn() && ai,
    bypass: state.bypass && ai,
    remoteControl: state.remote && kind === 'claude',
    model: ai ? currentModel()?.id || null : null,
    attachments: ai ? state.attachments : [],
  });
  if (ai) state.attachments = [];
  renderAttachments();
  flash($('composer'), 'sent');
  if (text) state.history.push(raw);
  state.recall = -1;
  task.value = '';
  grow();
}

function onKey(event) {
  if (event.key === 'Enter' && !event.shiftKey && !event.altKey) {
    submit(event);
  } else if (event.key === 'Enter' && event.altKey) {
    event.preventDefault();
    const task = $('task');
    task.setRangeText('\n', task.selectionStart, task.selectionEnd, 'end');
    grow();
  } else if (event.key === 'Tab') {
    event.preventDefault();
    if (!event.shiftKey && completeTyped()) return;
    cycleWho(event.shiftKey ? -1 : 1);
  } else if (event.key === 'Escape' && !$('palette').hidden) {
    event.preventDefault();
    $('palette').hidden = true;
  } else if ((event.key === 'ArrowDown' || event.key === 'ArrowUp') && !event.ctrlKey && !$('palette').hidden) {
    event.preventDefault();
    moveSuggestion(event.key === 'ArrowDown' ? 1 : -1);
  } else if (event.ctrlKey && event.key === 'ArrowUp') {
    event.preventDefault();
    recall(-1);
  } else if (event.ctrlKey && event.key === 'ArrowDown') {
    event.preventDefault();
    recall(1);
  }
}

export function renderComposer() {
  const who = currentWho();
  $('who').replaceChildren(
    h('span', { class: 'value', style: `color:${kindColor(who)}` }, label(who)),
    svg(ICON_CHEVRON_DOWN),
  );
  $('who').setAttribute('aria-label', `${t('Who runs the task')}: ${label(who)}`);
  $('who').title = t('Who runs the task');
  renderModel();
  $('where').replaceChildren(h('span', { class: 'value path' }, currentWhere()));
  $('where').setAttribute('aria-label', `${t('Where the task runs')}: ${currentWhere()}`);
  $('where').title = currentWhere();
  renderOptionsButton();
  renderAttachments();
  renderOptionRows();
}

export function initComposer() {
  $('composer').addEventListener('submit', submit);
  $('task').addEventListener('keydown', onKey);
  $('task').addEventListener('input', () => {
    grow();
    renderPalette();
  });
  $('who').addEventListener('click', (event) => {
    event.stopPropagation();
    if (!menuShown()) openWhoMenu();
    else closeMenu();
  });
  $('model').addEventListener('click', (event) => {
    event.stopPropagation();
    if (!menuShown()) openModelMenu();
    else closeMenu();
  });
  $('where').addEventListener('click', () =>
    pickFolder(currentWhere(), (path) => {
      setWhere(path);
      renderComposer();
    }),
  );
  $('options').replaceChildren(svg(ICON_OPTIONS));
  $('options').addEventListener('click', (event) => {
    event.stopPropagation();
    if (!menuShown()) openOptionsMenu(event);
    else closeMenu();
  });
  $('attach').replaceChildren(svg(ICON_ATTACH));
  $('attach').addEventListener('click', chooseFiles);
  onFileDrop(onDrop);
  document.addEventListener('click', (event) => {
    if (!$('menu').contains(event.target)) closeMenu();
  });
  document.addEventListener('keydown', (event) => {
    if (event.key === 'Escape') closeMenu();
  });
}
