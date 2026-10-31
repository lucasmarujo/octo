import { $, h, svg } from './dom.js';
import { ask, confirm } from './dialogs.js';
import { SPINNER, activity, alive, duration, glyph, kindColor, label } from './format.js';
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
  if (agent.starting) return { text: 'Starting…', tone: 't-faint' };
  if (agent.state === 'lost') return { text: 'Interrupted by a crash or a reboot', tone: 't-red' };
  if (agent.state === 'exited') return { text: agent.exit || 'Ended', tone: 't-faint' };
  const since = agent.stateSince ? duration(at - agent.stateSince) : null;
  if (agent.kind === 'terminal') return { text: agent.state === 'working' ? 'Printing output' : 'Quiet', tone: 't-muted' };
  if (agent.state === 'working') return { text: `Working for ${duration(at - (agent.turnStarted || agent.stateSince))}`, tone: 't-muted' };
  if (agent.state === 'paused') return { text: `Waiting for your answer · ${since}`, tone: 't-yellow' };
  if (agent.state === 'done') return { text: `Done ${since} ago`, tone: 't-green' };
  return { text: since ? `Idle for ${since}` : 'Idle', tone: 't-muted' };
}

export async function rename(agent) {
  const name = await ask({ title: 'Rename the agent', value: agent.name, action: 'Rename' });
  if (name && name !== agent.name) send({ type: 'rename', agent: agent.id, name });
}

async function stop(agent) {
  const sure = await confirm({
    title: `Stop ${agent.name}?`,
    text: 'The agent process ends. Its card and tab stay until you remove it.',
    action: 'Stop',
    danger: true,
  });
  if (sure) send({ type: 'stop', agent: agent.id });
}

async function remove(agent) {
  const worktree = agent.branch ? ` Its worktree and the ${agent.branch} branch are deleted too.` : '';
  const sure = await confirm({
    title: `Remove ${agent.name}?`,
    text: `The agent and its recorded output are deleted.${worktree}`,
    action: 'Remove',
    danger: true,
  });
  if (sure) send({ type: 'remove', agent: agent.id });
}

export async function merge(agent) {
  const sure = await confirm({
    title: `Merge ${agent.branch}?`,
    text: 'Whatever is pending on the branch is committed and merged into the current branch of the repository. A conflict undoes the merge and leaves the repository as it was.',
    action: 'Merge',
  });
  if (sure) send({ type: 'merge', agent: agent.id });
}

export async function restart(agent) {
  if (!alive(agent)) {
    send({ type: 'restart', agent: agent.id });
    return;
  }
  const sure = await confirm({
    title: `Restart ${agent.name}?`,
    text: 'The agent stops and starts again in the same folder, picking up its conversation where it was.',
    action: 'Restart',
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
    button('Rename', rename),
    agent.kind === 'terminal' && !alive(agent) ? null : button('Restart', restart),
    agent.branch ? button('Cross-review', crossReview) : null,
    alive(agent) ? button('Stop', stop, 'warn') : button('Remove', remove, 'warn'),
  ].filter(Boolean);
}

function section(title, aside, ...body) {
  return h('section', { class: 'detail' }, h('div', { class: 'detail-head' }, h('span', { class: 'caps' }, title), aside ? h('span', { class: 'detail-aside' }, aside) : null), body);
}

function subagentsSection(list) {
  const running = list.length ? `${list.length} running` : null;
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
    : [h('span', { class: 'detail-note' }, 'No subagent is running.')];
  return section('Subagents', running, ...body);
}

function changesSection(agent, info) {
  const busy = agent.state === 'working' || agent.state === 'paused';
  const files = info && info.changes ? info.changes : [];
  const plus = files.reduce((sum, file) => sum + file.insertions, 0);
  const minus = files.reduce((sum, file) => sum + file.deletions, 0);
  const totals = files.length
    ? [`${files.length} file${files.length === 1 ? '' : 's'} · `, h('span', { class: 't-green' }, `+${plus}`), ' ', h('span', { class: 't-red' }, `−${minus}`)]
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
      : h('span', { class: 'detail-note' }, 'No changes on the branch yet.')
    : h('span', { class: 'detail-note' }, 'Reading the changes…');
  const overlaps = info ? info.overlaps : [];
  return section(
    'Changes',
    totals,
    list,
    ...overlaps.map((overlap) => h('span', { class: 'overlap t-yellow', title: overlap.paths.join('\n') }, `⚠ ${overlap.name} also changes ${overlap.paths[0]}${overlap.paths.length > 1 ? ` and ${overlap.paths.length - 1} more` : ''}`)),
    h(
      'div',
      { class: 'detail-actions' },
      h('button', { class: 'button', type: 'button', onclick: () => send({ type: 'diff', agent: agent.id }) }, 'Open diff'),
      h('button', { class: 'button merge', type: 'button', disabled: busy, onclick: () => merge(agent) }, 'Merge'),
    ),
    busy ? h('span', { class: 'detail-note' }, 'Merge unlocks when the agent stops or is done.') : null,
  );
}

export function testLine(test, at) {
  const ago = test.finishedAt ? ` · ${duration(at - test.finishedAt)} ago` : '';
  if (test.state === 'running') return { mark: SPINNER[0], tone: 't-yellow', text: 'Running after the last turn…' };
  if (test.state === 'passed') return { mark: '✓', tone: 't-green', text: `Passed after the last turn${ago}` };
  if (test.state === 'failed') return { mark: '✗', tone: 't-red', text: `Failed${test.code === null ? '' : ` (exit ${test.code})`} after the last turn${ago}` };
  return { mark: '!', tone: 't-yellow', text: 'Not trusted yet: the command changed since you trusted it' };
}

function testSection(agent, test) {
  const line = testLine(test, now());
  return section(
    'Test',
    TEST_SOURCE,
    h(
      'div',
      { class: 'test' },
      h('span', { class: `test-mark ${line.tone}` }, line.mark),
      h('div', { class: 'test-text' }, h('span', { class: 'test-command' }, test.command), h('span', { class: 'test-state' }, line.text)),
      h('button', { class: 'link-button small', type: 'button', onclick: () => send({ type: 'tests', agent: agent.id }) }, 'Output'),
    ),
  );
}

function details(agent, info) {
  const parts = [subagentsSection(info ? info.subagents : [])];
  if (agent.branch) parts.push(changesSection(agent, info));
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
  if (agent.startedAt && alive(agent)) meta.push(h('span', { class: 'meta-item t-faint' }, `alive ${duration(now() - agent.startedAt)}`));
  $('agent-meta').replaceChildren(...meta);
}

export function renderStatus(agent) {
  const other = nextAttention(agent ? agent.id : null);
  const left = other
    ? other.state === 'paused'
      ? h('span', { class: 'status-alert' }, h('span', { class: 'dot' }), `${other.name} is waiting for you`, h('span', { class: 'kbd' }, 'alt N opens it'))
      : h('span', { class: 'status-done' }, `✓ ${other.name} is done`, h('span', { class: 'kbd' }, 'alt N opens it'))
    : h('span', { class: 't-faint' }, activity(agent || { state: 'idle', activity: '' }).text);
  $('tab-status').replaceChildren(
    left,
    h('span', { class: 'grow' }),
    h('span', { class: 'kbd' }, 'drag to select · ctrl+click opens links · ctrl ] overview'),
  );
}
