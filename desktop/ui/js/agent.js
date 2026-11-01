import { $, h, svg } from './dom.js';
import { ask, confirm } from './dialogs.js';
import { SPINNER, alive, duration, glyph, kindColor, label, plural } from './format.js';
import { hubText, t } from './i18n.js';
import { ICON_BRANCH } from './icons.js';
import { agents, now, state } from './state.js';
import { send } from './transport.js';

const ATTENTION = ['paused', 'done'];
const TEST_SOURCE = '.orkai.json';

export const restarts = new Set();

export function nextAttention(skip) {
  const waiting = (name) =>
    agents()
      .filter((agent) => agent.state === name && !agent.starting && agent.id !== skip)
      .sort((a, b) => (a.stateSince || 0) - (b.stateSince || 0));
  for (const name of ATTENTION) {
    const [first] = waiting(name);
    if (first) return first;
  }
  return null;
}

export function shortPath(path) {
  const home = state.home;
  if (!home || !path) return path || '';
  const normal = path.replaceAll('\\', '/');
  const base = home.replaceAll('\\', '/').replace(/\/$/, '');
  return normal.toLowerCase().startsWith(base.toLowerCase()) ? `~${normal.slice(base.length)}` : normal;
}

function stateLine(agent) {
  const at = now();
  if (agent.starting) return { text: t('Starting…'), tone: 't-faint' };
  if (agent.state === 'lost') return { text: t('Interrupted by a crash or a reboot'), tone: 't-red' };
  if (agent.state === 'exited') return { text: agent.exit ? hubText(agent.exit) : t('Ended'), tone: 't-faint' };
  const since = agent.stateSince ? duration(at - agent.stateSince) : null;
  if (agent.kind === 'terminal') return { text: agent.state === 'working' ? t('Printing output') : t('Quiet'), tone: 't-muted' };
  if (agent.state === 'working') return { text: t('Working for {time}', { time: duration(at - (agent.turnStarted || agent.stateSince)) }), tone: 't-muted' };
  if (agent.state === 'paused') return { text: t('Waiting for your answer · {time}', { time: since }), tone: 't-yellow' };
  if (agent.state === 'done') return { text: t('Done {time} ago', { time: since }), tone: 't-green' };
  return { text: since ? t('Idle for {time}', { time: since }) : t('Idle'), tone: 't-muted' };
}

export async function rename(agent) {
  const name = await ask({ title: t('Rename the agent'), value: agent.name, action: t('Rename') });
  if (name && name !== agent.name) send({ type: 'rename', agent: agent.id, name });
}

async function stop(agent) {
  const sure = await confirm({
    title: t('Stop {name}?', { name: agent.name }),
    text: t('The agent process ends. Its card and tab stay until you remove it.'),
    action: t('Stop'),
    danger: true,
  });
  if (sure) send({ type: 'stop', agent: agent.id });
}

async function remove(agent) {
  const worktree = agent.branch ? ` ${t('Its worktree and the {branch} branch are deleted too.', { branch: agent.branch })}` : '';
  const sure = await confirm({
    title: t('Remove {name}?', { name: agent.name }),
    text: `${t('The agent and its recorded output are deleted.')}${worktree}`,
    action: t('Remove'),
    danger: true,
  });
  if (sure) send({ type: 'remove', agent: agent.id });
}

export async function merge(agent) {
  if (!agent.branch) return commit(agent);
  const sure = await confirm({
    title: t('Merge {branch}?', { branch: agent.branch }),
    text: t('Whatever is pending on the branch is committed and merged into the current branch of the repository. A conflict undoes the merge and leaves the repository as it was.'),
    action: t('Merge'),
  });
  if (sure) send({ type: 'merge', agent: agent.id });
}

async function commit(agent) {
  const sure = await confirm({
    title: t('Commit what is pending?'),
    text: t('Everything not yet committed in the repository of {path} is committed to its current branch, including changes made by you or by other agents in that folder.', { path: shortPath(agent.cwd) }),
    action: t('Commit'),
  });
  if (sure) send({ type: 'merge', agent: agent.id });
}

export async function restart(agent) {
  if (!alive(agent)) {
    send({ type: 'restart', agent: agent.id });
    return;
  }
  const sure = await confirm({
    title: t('Restart {name}?', { name: agent.name }),
    text: t('The agent stops and starts again in the same folder, picking up its conversation where it was.'),
    action: t('Restart'),
  });
  if (!sure) return;
  restarts.add(agent.id);
  send({ type: 'stop', agent: agent.id });
}

export function restartWhenStopped(list) {
  for (const agent of list) {
    if (!restarts.has(agent.id) || alive(agent)) continue;
    restarts.delete(agent.id);
    send({ type: 'restart', agent: agent.id });
  }
}

function crossReview(agent) {
  send({ type: 'crossReview', agent: agent.id });
}

function actions(agent) {
  const button = (text, onclick, tone = '') => h('button', { class: `button agent-action ${tone}`, type: 'button', onclick: () => onclick(agent) }, text);
  return [
    button(t('Rename'), rename),
    button(t('Restart'), restart),
    agent.branch ? button(t('Cross-review'), crossReview) : null,
    alive(agent) ? button(t('Stop'), stop, 'warn') : button(t('Remove'), remove, 'warn'),
  ].filter(Boolean);
}

function section(title, aside, ...body) {
  return h('section', { class: 'detail' }, h('div', { class: 'detail-head' }, h('span', { class: 'caps' }, title), aside ? h('span', { class: 'detail-aside' }, aside) : null), body);
}

function subagentsSection(list) {
  const running = list.length ? t('{count} running', { count: list.length }) : null;
  const body = list.length
    ? list.map((sub) =>
        h(
          'div',
          { class: 'subagent' },
          h(
            'div',
            { class: 'subagent-top' },
            h('span', { class: 'glyph t-yellow', dataset: { spin: '1' } }, SPINNER[state.frame % SPINNER.length]),
            h('span', { class: 'subagent-role' }, sub.role),
            h('span', { class: 'subagent-meta' }, [duration(now() - sub.startedAt), sub.activity].filter(Boolean).join(' · ')),
          ),
          sub.description ? h('span', { class: 'subagent-text' }, sub.description) : null,
        ),
      )
    : [h('span', { class: 'detail-note' }, t('No subagent is running.'))];
  return section(t('Subagents'), running, ...body);
}

function overlapText(overlap) {
  const values = { name: overlap.name, path: overlap.paths[0], count: overlap.paths.length - 1 };
  return overlap.paths.length > 1 ? t('⚠ {name} also changes {path} and {count} more', values) : t('⚠ {name} also changes {path}', values);
}

function changesSection(agent, info) {
  const inFolder = !agent.branch;
  const busy = agent.state === 'working' || agent.state === 'paused';
  const files = info && info.changes ? info.changes : [];
  const plus = files.reduce((sum, file) => sum + file.insertions, 0);
  const minus = files.reduce((sum, file) => sum + file.deletions, 0);
  const totals = files.length
    ? [`${plural(files.length, t('file'), t('files'))} · `, h('span', { class: 't-green' }, `+${plus}`), ' ', h('span', { class: 't-red' }, `−${minus}`)]
    : null;
  const list = info && info.changes
    ? files.length
      ? h('div', { class: 'changes' }, ...files.map((file) =>
          h(
            'div',
            { class: 'change', title: file.path },
            h('span', { class: 'change-path' }, file.path),
            h('span', { class: `change-plus ${file.insertions ? 't-green' : 't-faint'}` }, `+${file.insertions}`),
            h('span', { class: `change-minus ${file.deletions ? 't-red' : 't-faint'}` }, file.deletions ? `−${file.deletions}` : '0'),
          ),
        ))
      : h('span', { class: 'detail-note' }, inFolder ? t('Nothing pending in the folder.') : t('No changes on the branch yet.'))
    : h('span', { class: 'detail-note' }, t('Reading the changes…'));
  const overlaps = info ? info.overlaps : [];
  return section(
    t('Changes'),
    totals,
    inFolder ? h('span', { class: 'detail-note' }, t('Everything pending in the folder, not only what this agent changed.')) : null,
    list,
    ...overlaps.map((overlap) => h('span', { class: 'overlap t-yellow', title: overlap.paths.join('\n') }, overlapText(overlap))),
    h(
      'div',
      { class: 'detail-actions' },
      h('button', { class: 'button', type: 'button', onclick: () => send({ type: 'diff', agent: agent.id }) }, t('Open diff')),
      inFolder
        ? h('button', { class: 'button merge', type: 'button', disabled: busy || !files.length, onclick: () => commit(agent) }, t('Commit'))
        : h('button', { class: 'button merge', type: 'button', disabled: busy, onclick: () => merge(agent) }, t('Merge')),
    ),
    busy ? h('span', { class: 'detail-note' }, inFolder ? t('Commit unlocks when the agent stops or is done.') : t('Merge unlocks when the agent stops or is done.')) : null,
  );
}

function commitsSection(agent, commits) {
  const own = commits.filter((commit) => commit.onBranch).length;
  return section(
    t('Commits'),
    agent.branch && own ? t('{count} on this branch', { count: own }) : null,
    h('div', { class: 'commits' }, ...commits.map((commit) =>
      h(
        'div',
        { class: commit.onBranch ? 'commit on-branch' : 'commit', title: `${commit.hash} · ${commit.author}\n${commit.subject}` },
        h('span', { class: 'commit-hash' }, commit.hash),
        h('span', { class: 'commit-subject' }, commit.subject),
        h('span', { class: 'commit-time' }, t('{time} ago', { time: duration(now() - commit.at) })),
      ),
    )),
  );
}

export function testLine(test, at) {
  const ago = test.finishedAt ? ` · ${t('{time} ago', { time: duration(at - test.finishedAt) })}` : '';
  if (test.state === 'running') return { mark: SPINNER[0], tone: 't-yellow', text: t('Running after the last turn…') };
  if (test.state === 'passed') return { mark: '✓', tone: 't-green', text: `${t('Passed after the last turn')}${ago}` };
  if (test.state === 'failed') {
    const failed = test.code === null ? t('Failed after the last turn') : t('Failed (exit {code}) after the last turn', { code: test.code });
    return { mark: '✗', tone: 't-red', text: `${failed}${ago}` };
  }
  return { mark: '!', tone: 't-yellow', text: t('Not trusted yet: the command changed since you trusted it') };
}

function testSection(agent, test) {
  const line = testLine(test, now());
  return section(
    t('Test'),
    TEST_SOURCE,
    h(
      'div',
      { class: 'test' },
      h('span', { class: `test-mark ${line.tone}` }, line.mark),
      h('div', { class: 'test-text' }, h('span', { class: 'test-command' }, test.command), h('span', { class: 'test-state' }, line.text)),
      h('button', { class: 'link-button small', type: 'button', onclick: () => send({ type: 'tests', agent: agent.id }) }, t('Output')),
    ),
  );
}

function details(agent, info) {
  const parts = [subagentsSection(info ? info.subagents : [])];
  if (agent.branch || (info && info.changes)) parts.push(changesSection(agent, info));
  if (info && info.commits) parts.push(commitsSection(agent, info.commits));
  if (info && info.test) parts.push(testSection(agent, info.test));
  return parts;
}

export function detailsSignature(agent, info) {
  return JSON.stringify([agent.id, agent.branch, agent.state, info, Math.floor(now() / 10_000)]);
}

function refresh(host, signature, build) {
  if (host.dataset.signature === signature) return;
  host.replaceChildren(...build());
  host.dataset.signature = signature;
}

export function renderAgent(agent) {
  renderAgentHead(agent);
  refresh($('agent-actions'), JSON.stringify([agent.id, agent.name, agent.kind, agent.branch, alive(agent)]), () => actions(agent));
  const info = state.details && state.details.agent === agent.id ? state.details : null;
  refresh($('details'), detailsSignature(agent, info), () => details(agent, info));
  renderStatus(agent);
}

export function renderAgentHead(agent) {
  const mark = glyph(agent, state.frame);
  const line = stateLine(agent);
  $('agent-title').replaceChildren(
    h('span', { class: `agent-glyph ${mark.tone}`, dataset: mark.spin ? { spin: '1' } : {} }, mark.text),
    h('span', { class: 'agent-name' }, agent.name),
    h('span', { class: 'kind-badge', style: `color:${kindColor(agent.kind)};background:color-mix(in srgb, ${kindColor(agent.kind)} 16%, transparent)` }, label(agent.kind)),
    h('span', { class: `agent-state ${line.tone}` }, line.text),
  );
  const meta = [
    h('span', { class: 'meta-item' }, h('span', { class: 'swatch', style: `background:${agent.color}` }), agent.project),
  ];
  if (agent.branch) meta.push(h('span', { class: 'meta-item' }, svg(ICON_BRANCH), agent.branch));
  meta.push(h('span', { class: 'meta-item meta-path selectable', title: agent.cwd }, shortPath(agent.cwd)));
  if (agent.startedAt && alive(agent)) meta.push(h('span', { class: 'meta-item t-faint' }, t('alive {time}', { time: duration(now() - agent.startedAt) })));
  $('agent-meta').replaceChildren(...meta);
}

export function renderStatus(agent) {
  const other = nextAttention(agent ? agent.id : null);
  const left = other
    ? other.state === 'paused'
      ? h('span', { class: 'status-alert' }, h('span', { class: 'dot' }), t('{name} is waiting for you', { name: other.name }), h('span', { class: 'kbd' }, t('alt N opens it')))
      : h('span', { class: 'status-done' }, t('✓ {name} is done', { name: other.name }), h('span', { class: 'kbd' }, t('alt N opens it')))
    : h('span', null);
  $('tab-status').replaceChildren(
    left,
    h('span', { class: 'grow' }),
    h('span', { class: 'kbd' }, t('drag to select · ctrl+click opens links · ctrl ] overview')),
  );
}
