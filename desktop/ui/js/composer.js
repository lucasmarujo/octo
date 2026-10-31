import { $, h, svg } from './dom.js';
import { completeCommand, matchingCommands, parseCommand } from './commands.js';
import { closeMenu, openMenu, pickFolder } from './dialogs.js';
import { kindColor, label } from './format.js';
import { t } from './i18n.js';
import { ICON_BRANCH, ICON_CHEVRON_DOWN, ICON_SHIELD } from './icons.js';
import {
  currentWhere,
  currentWho,
  installedKinds,
  setBypass,
  setWhere,
  setWho,
  setWorktree,
  state,
  worktreeOn,
} from './state.js';
import { toast } from './toast.js';
import { send } from './transport.js';

const MAX_HEIGHT = 160;
const MAX_SUGGESTIONS = 8;
const SKILLS_TTL_MS = 5000;

let runCommand = () => false;

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

function typedWord() {
  const text = $('task').value;
  if (!text.startsWith('/') || text.includes('\n')) return null;
  const space = text.search(/\s/);
  return space === -1 ? text : null;
}

export function renderPalette() {
  const palette = $('palette');
  const word = typedWord();
  if (word === null) {
    palette.hidden = true;
    return;
  }
  askSkills();
  const commands = matchingCommands(word).map(([usage, about]) => ({ text: usage, about, kind: 'command' }));
  const skills = skillNames()
    .filter((skill) => `/${skill}`.startsWith(word))
    .map((skill) => ({ text: `/${skill}`, about: `skill · ${label(currentWho())}`, kind: 'skill' }));
  const items = [...commands, ...skills].slice(0, MAX_SUGGESTIONS);
  palette.hidden = items.length === 0;
  palette.replaceChildren(
    ...items.map((item) =>
      h(
        'button',
        {
          class: `palette-item ${item.kind}`,
          type: 'button',
          role: 'option',
          onmousedown: (event) => event.preventDefault(),
          onclick: () => {
            const usage = item.text.split(/\s/)[0];
            $('task').value = `${usage}${item.text.includes(' ') || item.kind === 'skill' ? ' ' : ''}`;
            $('task').focus();
            renderPalette();
          },
        },
        h('span', { class: 'palette-usage' }, item.text),
        h('span', { class: 'palette-about' }, item.about),
      ),
    ),
  );
}

function completeTyped() {
  const word = typedWord();
  if (word === null) return false;
  const skills = skillNames().map((skill) => `/${skill}`).filter((skill) => skill.startsWith(word));
  const completed = completeCommand(word, skills);
  if (completed) {
    $('task').value = completed;
    renderPalette();
  }
  return true;
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
          $('task').focus();
        },
      },
      h('span', { style: `color:${kindColor(kind)}` }, label(kind)),
      kind === current ? h('span', { class: 'check' }, '✓') : null,
    ),
  );
  openMenu($('who'), items, true);
}

function cycleWho(step) {
  const kinds = installedKinds();
  if (!kinds.length) return;
  const at = kinds.indexOf(currentWho());
  setWho(kinds[(at + step + kinds.length) % kinds.length]);
  renderComposer();
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
  } else if (kind !== 'terminal' && !text) {
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
  });
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
    h('span', { class: 'key' }, t('who')),
    h('span', { class: 'value', style: `color:${kindColor(who)}` }, label(who)),
    svg(ICON_CHEVRON_DOWN),
  );
  $('where').replaceChildren(h('span', { class: 'key' }, t('where')), h('span', { class: 'value path' }, currentWhere()));
  $('where').title = currentWhere();
  const worktree = $('worktree');
  worktree.setAttribute('aria-pressed', String(worktreeOn()));
  worktree.replaceChildren(svg(ICON_BRANCH), t('Worktree'));
  worktree.title = t('Each new AI agent runs in its own git worktree and orkai/<name> branch');
  const bypass = $('bypass');
  bypass.classList.add('danger');
  bypass.setAttribute('aria-pressed', String(state.bypass));
  bypass.replaceChildren(svg(ICON_SHIELD), t('Bypass'));
  bypass.title = t('New AI agents run tools, commands and edits without asking. Only in folders you trust.');
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
    if ($('menu').hidden) openWhoMenu();
    else closeMenu();
  });
  $('where').addEventListener('click', () =>
    pickFolder(currentWhere(), (path) => {
      setWhere(path);
      renderComposer();
    }),
  );
  $('worktree').addEventListener('click', () => {
    setWorktree(!worktreeOn());
    renderComposer();
  });
  $('bypass').addEventListener('click', () => {
    setBypass(!state.bypass);
    renderComposer();
  });
  document.addEventListener('click', (event) => {
    if (!$('menu').contains(event.target)) closeMenu();
  });
  document.addEventListener('keydown', (event) => {
    if (event.key === 'Escape') closeMenu();
  });
}
