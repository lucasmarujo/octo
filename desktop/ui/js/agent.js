import { $, h, svg } from './dom.js';
import { ask, confirm } from './dialogs.js';
import { activity, alive, duration, glyph, kindColor, label } from './format.js';
import { ICON_BRANCH } from './icons.js';
import { agents, now, state } from './state.js';
import { send } from './transport.js';

const ATTENTION = ['paused', 'done'];

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

async function merge(agent) {
  const sure = await confirm({
    title: `Merge ${agent.branch}?`,
    text: 'Whatever is pending on the branch is committed and merged into the current branch of the repository. A conflict undoes the merge and leaves the repository as it was.',
    action: 'Merge',
  });
  if (sure) send({ type: 'merge', agent: agent.id });
}

function actions(agent) {
  const buttons = [h('button', { class: 'button', type: 'button', onclick: () => rename(agent) }, 'Rename')];
  if (agent.state === 'lost') {
    buttons.push(h('button', { class: 'button', type: 'button', onclick: () => send({ type: 'recover', session: agent.session }) }, 'Recover'));
  }
  buttons.push(
    alive(agent)
      ? h('button', { class: 'button warn', type: 'button', onclick: () => stop(agent) }, 'Stop')
      : h('button', { class: 'button warn', type: 'button', onclick: () => remove(agent) }, 'Remove'),
  );
  return buttons;
}

function section(title, ...body) {
  return h('section', { class: 'detail' }, h('div', { class: 'detail-head' }, h('span', { class: 'caps' }, title)), body);
}

function details(agent) {
  const parts = [];
  if (agent.task) parts.push(section('Task', h('p', { class: 'detail-text selectable' }, agent.task)));
  if (agent.branch) {
    const busy = agent.state === 'working' || agent.state === 'paused';
    parts.push(
      section(
        'Worktree',
        h('div', { class: 'detail-branch' }, svg(ICON_BRANCH), h('span', null, agent.branch)),
        h(
          'div',
          { class: 'detail-actions' },
          h('button', { class: 'button', type: 'button', onclick: () => send({ type: 'diff', agent: agent.id }) }, 'Open diff'),
          h('button', { class: 'button primary', type: 'button', disabled: busy, onclick: () => merge(agent) }, 'Merge'),
        ),
        busy ? h('span', { class: 'detail-note' }, 'Merge unlocks when the agent stops or is done.') : null,
      ),
    );
  }
  parts.push(section('Folder', h('p', { class: 'detail-path selectable' }, agent.cwd)));
  return parts;
}

function refresh(host, signature, build) {
  if (host.dataset.signature === signature) return;
  host.replaceChildren(...build());
  host.dataset.signature = signature;
}

export function renderAgent(agent) {
  renderAgentHead(agent);
  const busy = agent.state === 'working' || agent.state === 'paused';
  refresh($('agent-actions'), JSON.stringify([agent.id, agent.name, agent.state === 'lost', alive(agent)]), () => actions(agent));
  refresh($('details'), JSON.stringify([agent.id, agent.task, agent.branch, agent.cwd, busy]), () => details(agent));
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
    h('span', { class: 'kbd' }, 'drag selects · ctrl c copies · ctrl v pastes · ctrl ] overview'),
  );
}
