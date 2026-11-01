import { $, h, keep, place, svg } from './dom.js';
import { merge } from './agent.js';
import { closeMenu, confirm, openMenu } from './dialogs.js';
import {
  COLUMNS,
  SPINNER,
  activity,
  alive,
  changeParts,
  column,
  elapsed,
  glyph,
  idleRunning,
  idleStopped,
  kindColor,
  label,
  nameTone,
  plural,
  subagentLine,
  testMark,
} from './format.js';
import { t } from './i18n.js';
import { ICON_BRANCH, ICON_CHEVRON_DOWN, ICON_STOP, ICON_TRASH } from './icons.js';
import { sleeping } from './kai.js';
import { glide, motion, reducedMotion, rise, slideThumb } from './motion.js';
import { currentProject, inSession, now, projectsInSession, setProject, state, visible } from './state.js';
import { send } from './transport.js';

async function stopAgent(agent) {
  const sure = await confirm({
    title: t('Stop {name}?', { name: agent.name }),
    text: t('The agent process ends. Its card stays here until you remove it.'),
    action: t('Stop'),
    danger: true,
  });
  if (sure) send({ type: 'stop', agent: agent.id });
}

async function removeAgent(agent) {
  const worktree = agent.branch ? ` ${t('Its worktree and the {branch} branch are deleted too.', { branch: agent.branch })}` : '';
  const sure = await confirm({
    title: t('Remove {name}?', { name: agent.name }),
    text: `${t('The agent and its recorded output are deleted.')}${worktree}`,
    action: t('Remove'),
    danger: true,
  });
  if (sure) send({ type: 'remove', agent: agent.id });
}

export function stopOrRemove(agent) {
  return alive(agent) ? stopAgent(agent) : removeAgent(agent);
}

export function tools(agent) {
  const act = (work) => (event) => {
    event.stopPropagation();
    work(agent);
  };
  const button = alive(agent)
    ? h('button', { class: 'icon-button danger', type: 'button', title: t('Stop'), onclick: act(stopAgent) }, svg(ICON_STOP))
    : h('button', { class: 'icon-button danger', type: 'button', title: t('Remove'), onclick: act(removeAgent) }, svg(ICON_TRASH));
  return h('div', { class: 'card-tools' }, button);
}

function card(agent, handlers) {
  const mark = glyph(agent, state.frame);
  const time = elapsed(agent, now());
  const doing = activity(agent);
  const waiting = agent.state === 'paused' && !agent.starting;
  const classes = ['card', waiting ? 'waiting' : '', agent.state === 'lost' ? 'lost' : ''].filter(Boolean).join(' ');
  const text = waiting
    ? h('div', { class: 'card-text' }, h('span', { class: 'card-strong' }, t('Waiting for your answer')), h('span', { class: 't-muted' }, agent.activity))
    : h('div', { class: `card-text ${doing.tone}${agent.kind === 'terminal' ? ' mono' : ''}` }, doing.text);
  const open = handlers.open
    ? h(
        'div',
        { class: 'card-actions' },
        h('button', { class: 'button waiting', type: 'button', onclick: () => handlers.open(agent) }, t('Open agent')),
        h('span', { class: 'kbd' }, 'alt N'),
      )
    : null;
  const subagents = (agent.subagents || []).slice(0, 2).map((sub) => {
    const line = subagentLine(sub, now());
    return h('div', { class: 'card-sub' }, h('span', { class: 'card-sub-role' }, `↳ ${line.role}`), h('span', { class: 'card-sub-time' }, line.time), sub.activity ? h('span', { class: 't-faint' }, `· ${sub.activity}`) : null);
  });
  const parts = changeParts(agent.changes);
  const test = testMark(agent.test);
  const settled = agent.state === 'done' || agent.state === 'idle' || !alive(agent);
  const act = (work) => (event) => {
    event.stopPropagation();
    work();
  };
  const branch = agent.branch
    ? h(
        'div',
        { class: 'card-foot' },
        h('span', { class: `branch${parts ? ' counts' : ''}`, title: agent.branch }, svg(ICON_BRANCH), parts ? parts.map((part) => h('span', { class: part.tone }, part.text)) : agent.branch),
        h('span', { class: 'grow' }),
        test ? h('span', { class: test.tone }, test.text) : null,
        parts && settled && handlers.open
          ? h(
              'span',
              { class: 'card-foot-actions' },
              h('button', { class: 'button tiny', type: 'button', onclick: act(() => send({ type: 'diff', agent: agent.id })) }, t('Diff')),
              h('button', { class: 'button tiny primary', type: 'button', onclick: act(() => merge(agent)) }, t('Merge')),
            )
          : null,
      )
    : null;
  return h(
    'article',
    { class: classes, tabindex: '0', dataset: { agent: String(agent.id) }, onclick: handlers.open ? () => handlers.open(agent) : null },
    tools(agent),
    h(
      'div',
      { class: 'card-top' },
      h('span', { class: `glyph ${mark.tone}`, dataset: mark.spin ? { spin: '1' } : {} }, mark.text),
      h('span', { class: `card-name ${nameTone(agent)}` }, agent.name),
      h('span', { class: `card-time ${time ? time.tone : ''}` }, time ? time.text : ''),
    ),
    h(
      'div',
      { class: 'card-meta' },
      h('span', { class: 'swatch', style: `background:${agent.color}` }),
      h('span', { class: 'project' }, agent.project),
      h('span', { class: 't-faint' }, '·'),
      h('span', { style: `color:${kindColor(agent.kind)}` }, label(agent.kind)),
    ),
    text,
    subagents.length ? h('div', { class: 'card-subs' }, subagents) : null,
    waiting ? open : null,
    branch,
  );
}

function columnHead(def, count) {
  const mark = {
    needs: h('span', { class: 'dot' }),
    working: h('span', { class: 'glyph t-text', dataset: { spin: '1' } }, SPINNER[state.frame % SPINNER.length]),
    done: h('span', { class: 'glyph t-green' }, '✓'),
    idle: h('span', { class: 'glyph t-faint' }, '●'),
  }[def.key];
  const actions = def.key === 'idle'
    ? [
        h('button', { class: 'button tiny column-action', type: 'button', hidden: true, dataset: { action: 'stop-all' }, onclick: stopIdle }, t('Stop all')),
        h('button', { class: 'button tiny warn column-action', type: 'button', hidden: true, dataset: { action: 'remove-all' }, onclick: removeStopped }, t('Remove all')),
      ]
    : null;
  return h('div', { class: 'column-head' }, mark, h('span', { class: 'name' }, def.title), actions, h('span', { class: 'count' }, String(count)));
}

async function stopIdle() {
  const targets = idleRunning(visible());
  if (!targets.length) return;
  const sure = await confirm({
    title: t('Stop {agents}?', { agents: plural(targets.length, t('idle agent'), t('idle agents')) }),
    text: t('Their processes end. The cards stay in Idle until you remove them.'),
    action: t('Stop all'),
    danger: true,
  });
  if (!sure) return;
  for (const agent of targets) send({ type: 'stop', agent: agent.id });
}

async function removeStopped() {
  const targets = idleStopped(visible());
  if (!targets.length) return;
  const worktrees = targets.some((agent) => agent.branch) ? ` ${t('Their worktrees and branches are deleted too.')}` : '';
  const sure = await confirm({
    title: t('Remove {agents}?', { agents: plural(targets.length, t('stopped agent'), t('stopped agents')) }),
    text: `${t('The agents and their recorded output are deleted.')}${worktrees}`,
    action: t('Remove all'),
    danger: true,
  });
  if (!sure) return;
  for (const agent of targets) send({ type: 'remove', agent: agent.id });
}

function empty(session) {
  return h(
    'div',
    { class: 'empty' },
    sleeping(),
    h('span', { class: 'empty-title' }, t('No agents in {session} yet', { session })),
    h('span', null, t('Write a task below and press enter: the agent starts already working on it.')),
  );
}

const cards = new Map();
const columns = new Map();
let shownLayout = '';
let shownSession = null;

function cardPlaces(board) {
  const places = new Map();
  for (const node of board.querySelectorAll('.card[data-agent]')) {
    const box = node.getBoundingClientRect();
    if (box.width) places.set(node.dataset.agent, { box, column: node.closest('.column').dataset.column });
  }
  return places;
}

function settleCards(board, before) {
  const options = motion('--motion-slow');
  for (const node of board.querySelectorAll('.card[data-agent]')) {
    const was = before.get(node.dataset.agent);
    if (!was || was.column !== node.closest('.column').dataset.column) rise(node, options);
    else glide(node, was.box, options);
  }
}

function cardSignature(agent, handlers) {
  return JSON.stringify([agent.name, agent.kind, agent.state, agent.starting, agent.activity, agent.project, agent.color, agent.branch, agent.exit, agent.changes, agent.test, agent.subagents, Boolean(handlers.open)]);
}

export function renderBoard(handlers) {
  const board = $('board');
  const list = visible();
  if (!list.length) {
    const signature = `empty:${state.session}`;
    if (board.dataset.signature !== signature) {
      board.replaceChildren(empty(state.session));
      board.dataset.signature = signature;
    }
    cards.clear();
    shownLayout = '';
    return;
  }
  const layout = JSON.stringify(COLUMNS.map((def) => list.filter((agent) => column(agent) === def.key).map((agent) => agent.id)));
  const moved = shownLayout !== '' && layout !== shownLayout && shownSession === state.session && !reducedMotion();
  const before = moved ? cardPlaces(board) : null;
  const sections = COLUMNS.map((def) => {
    const members = list.filter((agent) => column(agent) === def.key);
    const nodes = members.length
      ? members.map((agent) => keep(cards, agent.id, cardSignature(agent, handlers), () => card(agent, handlers)))
      : [keep(columns, `empty:${def.key}`, '', () => h('div', { class: 'column-empty' }, def.empty))];
    const section = keep(columns, def.key, '', () =>
      h('section', { class: 'column', dataset: { column: def.key } }, columnHead(def, 0), h('div', { class: 'column-body' })),
    );
    section.querySelector('.count').textContent = String(members.length);
    const stopAll = section.querySelector('[data-action="stop-all"]');
    if (stopAll) stopAll.hidden = idleRunning(members).length === 0;
    const removeAll = section.querySelector('[data-action="remove-all"]');
    if (removeAll) removeAll.hidden = idleStopped(members).length === 0;
    place(section.querySelector('.column-body'), nodes);
    return section;
  });
  place(board, sections);
  board.dataset.signature = 'columns';
  if (before && before.size) settleCards(board, before);
  shownLayout = layout;
  shownSession = state.session;
  const ids = new Set(list.map((agent) => agent.id));
  for (const id of cards.keys()) if (!ids.has(id)) cards.delete(id);
}

export function renderHead() {
  const list = inSession();
  const projects = new Set(list.map((agent) => agent.project)).size;
  const waiting = list.filter((agent) => agent.state === 'paused' && !agent.starting).length;
  const lost = list.filter((agent) => agent.state === 'lost').length;
  $('session-title').textContent = state.session || '';
  const summary = $('session-summary');
  const across = { agents: plural(list.length, t('agent'), t('agents')), projects: plural(projects, t('project'), t('projects')) };
  const parts = [list.length ? t('{agents} across {projects}', across) : t('No agents yet')];
  if (waiting) parts.push(h('span', { class: 't-yellow' }, ` · ${waiting === 1 ? t('{count} needs you', { count: waiting }) : t('{count} need you', { count: waiting })}`));
  summary.replaceChildren(...parts);
  const recover = $('recover');
  recover.hidden = lost === 0;
  recover.textContent = t('Recover {agents}', { agents: plural(lost, t('interrupted agent'), t('interrupted agents')) });
  renderFilter(list);
  for (const button of $('views').children) button.setAttribute('aria-pressed', String(button.dataset.view === state.view));
  $('grid-size').hidden = state.view !== 'grid' || list.length === 0;
  for (const button of $('grid-size').children) button.setAttribute('aria-pressed', String(button.dataset.size === state.gridSize));
  slideThumb($('views'));
  slideThumb($('grid-size'));
}

function renderFilter(list) {
  const filter = $('project');
  const project = currentProject();
  const signature = JSON.stringify([project, list.length > 0]);
  if (filter.dataset.signature === signature) return;
  filter.dataset.signature = signature;
  filter.hidden = list.length === 0;
  filter.classList.toggle('active', project !== null);
  filter.replaceChildren(project || t('All projects'), svg(ICON_CHEVRON_DOWN));
}

export function openProjectMenu(onPick) {
  const list = inSession();
  const current = currentProject();
  const item = (project, text, count, color) =>
    h(
      'button',
      {
        class: `menu-item${project === current ? ' active' : ''}`,
        type: 'button',
        role: 'menuitem',
        onclick: () => {
          setProject(project);
          closeMenu();
          onPick();
        },
      },
      color ? h('span', { class: 'swatch', style: `background:${color}` }) : null,
      h('span', null, text),
      h('span', { class: 'check' }, project === current ? '✓' : String(count)),
    );
  const items = projectsInSession().map((project) => {
    const members = list.filter((agent) => agent.project === project);
    return item(project, project, members.length, members[0].color);
  });
  openMenu($('project'), [item(null, t('All projects'), list.length, null), ...items], false);
}

export function tickBoard() {
  const byId = new Map(visible().map((agent) => [String(agent.id), agent]));
  for (const node of $('board').querySelectorAll('[data-agent]')) {
    const agent = byId.get(node.dataset.agent);
    if (!agent) continue;
    const time = elapsed(agent, now());
    node.querySelector('.card-time').textContent = time ? time.text : '';
  }
}

export function spinBoard() {
  for (const node of $('board').querySelectorAll('[data-spin]')) node.textContent = SPINNER[state.frame % SPINNER.length];
}
