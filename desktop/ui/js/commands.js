import { merge, rename, restart } from './agent.js';
import { stopOrRemove } from './board.js';
import { ask, confirm, pickFolder } from './dialogs.js';
import { label } from './format.js';
import {
  currentWhere,
  currentWho,
  findAgent,
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

export const COMMANDS = [
  ['/where <path>', 'Folder where new tasks run'],
  ['/cd <path>', 'Same as /where'],
  ['/who <agent>', 'Who runs new tasks'],
  ['/session <name>', 'Switch to a session'],
  ['/new-session [name]', 'Create a session'],
  ['/rename-session <name>', 'Rename this session'],
  ['/delete-session', 'Delete an empty session'],
  ['/rename <name>', 'Rename the agent'],
  ['/kill', 'Stop the agent, or remove it once stopped'],
  ['/restart', 'Start the agent again'],
  ['/recover', 'Bring back interrupted agents'],
  ['/open <n>', 'Open agent number n'],
  ['/worktree [on|off]', 'One worktree per new agent'],
  ['/bypass [on|off]', 'New agents skip permission prompts'],
  ['/diff', 'Changes on the agent branch'],
  ['/merge', 'Merge the agent branch'],
  ['/cross-review [agent]', 'Another agent reviews the work'],
  ['/tests', 'Output of the last test run'],
  ['/trust', 'Run the tests of .orkai.json after each turn'],
  ['/race <agents> <task>', 'Agents race the same task in worktrees'],
  ['/compare', 'Compare the agents of a race'],
  ['/pick', 'Merge the winner, delete the others'],
  ['/pwa [off|new]', 'Pair your phone'],
  ['/help', 'How to use orkai'],
  ['/quit', 'Close the window, agents keep going'],
];

const ALIASES = { cd: 'where', s: 'session', ns: 'new-session', rm: 'kill', remove: 'kill', wt: 'worktree', phone: 'pwa', 'cross-review': 'cross-review' };
const name = (usage) => usage.split(/\s/)[0];

export function matchingCommands(word) {
  return COMMANDS.filter(([usage]) => usage.startsWith(word));
}

function commonPrefix(words) {
  if (!words.length) return '';
  let prefix = words[0];
  for (const word of words.slice(1)) {
    while (!word.startsWith(prefix)) prefix = prefix.slice(0, -1);
  }
  return prefix;
}

export function completeCommand(typed, skills) {
  const commands = matchingCommands(typed).map(([usage]) => usage);
  let completed;
  if (commands.length === 1 && !skills.length) completed = `${name(commands[0])}${commands[0].includes(' ') ? ' ' : ''}`;
  else if (!commands.length && skills.length === 1) completed = `${skills[0]} `;
  else completed = commonPrefix([...commands.map(name), ...skills]);
  return completed.length > typed.length ? completed : null;
}

export function parseCommand(text) {
  const line = text.trim();
  if (!line.startsWith('/') || line.includes('\n')) return null;
  const space = line.search(/\s/);
  const word = (space === -1 ? line.slice(1) : line.slice(1, space)).toLowerCase();
  const arg = space === -1 ? '' : line.slice(space).trim();
  return { name: ALIASES[word] || word, word, arg };
}

export const KIND_ALIASES = { cc: 'claude', kilocode: 'kilo', oc: 'opencode', agy: 'antigravity', gemini: 'antigravity', term: 'terminal', shell: 'terminal' };

export function parseKind(value) {
  const word = value.trim().toLowerCase();
  return KIND_ALIASES[word] || word;
}

function selected(ctx) {
  const agent = ctx.focused() || findAgent(state.selected);
  if (!agent) toast('Open or focus an agent first.', 'warn');
  return agent;
}

function toggle(arg, current, apply, title) {
  const on = { '': !current, on: true, off: false }[arg];
  if (on === undefined) return toast(`usage: /${title} [on|off]`, 'error');
  apply(on);
  return on;
}

async function renameSession(arg, ctx) {
  const to = arg || (await ask({ title: 'Rename the session', value: state.session, action: 'Rename' }));
  if (!to || to === state.session) return;
  state.pendingSession = to.trim();
  send({ type: 'renameSession', name: state.session, to });
  ctx.refresh();
}

async function deleteSession(arg) {
  const target = arg || state.session;
  const sure = await confirm({ title: `Delete the session ${target}?`, text: 'Only a session without agents can be deleted.', action: 'Delete', danger: true });
  if (sure) send({ type: 'deleteSession', name: target });
}

async function pick(agent) {
  const sure = await confirm({
    title: `Pick ${agent.name}?`,
    text: `${agent.branch} is merged into the repository and the other agents of the race are stopped and deleted with their worktrees.`,
    action: 'Pick',
    danger: true,
  });
  if (sure) send({ type: 'pick', agent: agent.id });
}

export function runCommand(text, ctx) {
  const command = parseCommand(text);
  if (!command) return false;
  const { name: which, word, arg } = command;
  const agentCommand = (work) => {
    const agent = selected(ctx);
    if (agent) work(agent);
  };
  switch (which) {
    case 'where':
      if (!arg) pickFolder(currentWhere(), (path) => (setWhere(path), ctx.refresh()));
      else {
        setWhere(arg);
        toast(`New tasks in ${state.session} will run in ${arg}`, 'info');
        ctx.refresh();
      }
      break;
    case 'who': {
      const kind = parseKind(arg);
      if (installedKinds().includes(kind)) {
        setWho(kind);
        ctx.refresh();
      } else toast(`usage: /who ${installedKinds().map(label).join('|')}`, 'error');
      break;
    }
    case 'session':
      if (state.overview.sessions.some((session) => session.name === arg)) ctx.selectSession(arg);
      else toast(arg ? `there is no session ${arg}` : 'usage: /session <name>', 'error');
      break;
    case 'new-session':
      if (arg) {
        state.pendingSession = arg;
        send({ type: 'session', name: arg });
      } else ctx.newSession();
      break;
    case 'rename-session':
      renameSession(arg, ctx);
      break;
    case 'delete-session':
      deleteSession(arg);
      break;
    case 'rename':
      agentCommand((agent) => (arg ? send({ type: 'rename', agent: agent.id, name: arg }) : rename(agent)));
      break;
    case 'kill':
      agentCommand(stopOrRemove);
      break;
    case 'restart':
      agentCommand(restart);
      break;
    case 'recover':
      send({ type: 'recover', session: state.session });
      break;
    case 'open': {
      const agent = findAgent(Number(arg));
      if (agent) ctx.openAgent(agent);
      else toast('usage: /open <agent number>', 'error');
      break;
    }
    case 'worktree':
      toggle(arg, worktreeOn(), setWorktree, 'worktree');
      ctx.refresh();
      break;
    case 'bypass':
      toggle(arg, state.bypass, setBypass, 'bypass');
      ctx.refresh();
      break;
    case 'diff':
      agentCommand((agent) => send({ type: 'diff', agent: agent.id }));
      break;
    case 'merge':
      agentCommand(merge);
      break;
    case 'cross-review':
      agentCommand((agent) => send({ type: 'crossReview', agent: agent.id, reviewer: arg }));
      break;
    case 'tests':
      agentCommand((agent) => send({ type: 'tests', agent: agent.id }));
      break;
    case 'trust':
      send({ type: 'trust', cwd: currentWhere() });
      break;
    case 'race':
      send({ type: 'race', session: state.session, cwd: currentWhere(), race: arg, bypass: state.bypass });
      break;
    case 'compare':
      agentCommand((agent) => send({ type: 'compare', agent: agent.id }));
      break;
    case 'pick':
      agentCommand(pick);
      break;
    case 'pwa':
      ctx.phone(arg);
      break;
    case 'help':
      ctx.help();
      break;
    case 'quit':
      ctx.quit();
      break;
    default:
      send({ type: 'skill', kind: currentWho(), session: state.session, cwd: currentWhere(), name: word, args: arg, worktree: worktreeOn(), bypass: state.bypass });
      break;
  }
  return true;
}
