import { merge, rename, restart } from './agent.js';
import { stopOrRemove } from './board.js';
import { ask, confirm, pickFolder } from './dialogs.js';
import { label } from './format.js';
import { t } from './i18n.js';
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
  ['/where <path>', t('Folder where new tasks run')],
  ['/cd <path>', t('Same as /where')],
  ['/who <agent>', t('Who runs new tasks')],
  ['/session <name>', t('Switch to a session')],
  ['/new-session [name]', t('Create a session')],
  ['/rename-session <name>', t('Rename this session')],
  ['/delete-session', t('Delete an empty session')],
  ['/rename <name>', t('Rename the agent')],
  ['/kill', t('Stop the agent, or remove it once stopped')],
  ['/restart', t('Start the agent again')],
  ['/recover', t('Bring back interrupted agents')],
  ['/open <n>', t('Open agent number n')],
  ['/worktree [on|off]', t('One worktree per new agent')],
  ['/bypass [on|off]', t('New agents skip permission prompts')],
  ['/diff', t('Changes on the agent branch')],
  ['/merge', t('Merge the agent branch')],
  ['/cross-review [agent]', t('Another agent reviews the work')],
  ['/tests', t('Output of the last test run')],
  ['/trust', t('Run the tests of .orkai.json after each turn')],
  ['/race <agents> <task>', t('Agents race the same task in worktrees')],
  ['/compare', t('Compare the agents of a race')],
  ['/pick', t('Merge the winner, delete the others')],
  ['/pwa [off|new]', t('Pair your phone')],
  ['/help', t('How to use orkai')],
  ['/quit', t('Close the window, agents keep going')],
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
  if (!agent) toast(t('Open or focus an agent first.'), 'warn');
  return agent;
}

function toggle(arg, current, apply, title) {
  const on = { '': !current, on: true, off: false }[arg];
  if (on === undefined) return toast(t('usage: {usage}', { usage: `/${title} [on|off]` }), 'error');
  apply(on);
  return on;
}

export async function renameSession(name, arg) {
  const to = arg || (await ask({ title: t('Rename the session'), value: name, action: t('Rename') }));
  if (!to || to === name) return false;
  if (name === state.session) state.pendingSession = to.trim();
  send({ type: 'renameSession', name, to });
  return true;
}

export async function deleteSession(name) {
  const sure = await confirm({ title: t('Delete the session {name}?', { name }), text: t('Only a session without agents can be deleted.'), action: t('Delete'), danger: true });
  if (sure) send({ type: 'deleteSession', name });
}

async function pick(agent) {
  const sure = await confirm({
    title: t('Pick {name}?', { name: agent.name }),
    text: t('{branch} is merged into the repository and the other agents of the race are stopped and deleted with their worktrees.', { branch: agent.branch }),
    action: t('Pick'),
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
        toast(t('New tasks in {session} will run in {path}', { session: state.session, path: arg }), 'info');
        ctx.refresh();
      }
      break;
    case 'who': {
      const kind = parseKind(arg);
      if (installedKinds().includes(kind)) {
        setWho(kind);
        ctx.refresh();
      } else toast(t('usage: {usage}', { usage: `/who ${installedKinds().map(label).join('|')}` }), 'error');
      break;
    }
    case 'session':
      if (state.overview.sessions.some((session) => session.name === arg)) ctx.selectSession(arg);
      else toast(arg ? t('there is no session {name}', { name: arg }) : t('usage: {usage}', { usage: '/session <name>' }), 'error');
      break;
    case 'new-session':
      if (arg) {
        state.pendingSession = arg;
        send({ type: 'session', name: arg });
      } else ctx.newSession();
      break;
    case 'rename-session':
      renameSession(state.session, arg).then((renamed) => renamed && ctx.refresh());
      break;
    case 'delete-session':
      deleteSession(arg || state.session);
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
      else toast(t('usage: /open <agent number>'), 'error');
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
