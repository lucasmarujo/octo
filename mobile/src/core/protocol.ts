import { wire } from './pairing';

export const { UNPAIRED, OUTDATED, REPLACED } = wire;

export type AgentKind = 'claude' | 'codex' | 'kilo' | 'opencode' | 'antigravity' | 'grok' | 'qwen' | 'kimi' | 'pi' | 'cursor' | 'terminal';
export type AgentState = 'idle' | 'working' | 'paused' | 'done' | 'exited' | 'lost';
export type TestState = 'running' | 'passed' | 'failed' | 'untrusted';
export type Tone = 'info' | 'success' | 'warn' | 'error';
export type Run = [text: string, fg: number, bg: number, flags: number];

export type SubagentView = {
  role: string;
  description: string;
  activity: string;
  startedAt: number;
};

export type ChangeCount = {
  files: number;
  insertions: number;
  deletions: number;
};

export type AgentView = {
  id: number;
  session: string;
  name: string;
  kind: AgentKind;
  state: AgentState;
  starting: boolean;
  activity: string;
  task: string;
  cwd: string;
  project: string;
  color: string;
  startedAt: number | null;
  stateSince: number | null;
  turnStarted: number | null;
  lastTurn: number | null;
  branch: string | null;
  exit: string | null;
  changes?: ChangeCount | null;
  test?: TestState | null;
  subagents?: SubagentView[];
  model?: string | null;
};

export type SessionView = {
  name: string;
  agents: number;
};

export type Overview = {
  host: string;
  now: number;
  session: string;
  sessions: SessionView[];
  agents: AgentView[];
  whereDir: string;
  dirs: Record<string, string>;
  worktree: boolean;
  kinds: AgentKind[];
};

export type UsageWindow = {
  label: string;
  used: number;
  resetsAt: number | null;
};

export type UsageEntry = {
  kind: AgentKind;
  windows: UsageWindow[];
  credits: number | null;
  problem: string | null;
};

export type Screen = {
  agent: number;
  rows: number;
  cols: number;
  full: boolean;
  cursor: [row: number, col: number] | null;
  scroll: number;
  lines: [index: number, runs: Run[]][];
};

export type Dir = {
  name: string;
  path: string;
  git: boolean;
};

export type FolderHit = Dir & {
  rel: string;
  repo: string | null;
};

export type Preview = {
  agent: number;
  lines: Run[][];
};

export type ChatItem =
  | { kind: 'user'; text: string }
  | { kind: 'text'; text: string }
  | { kind: 'step'; id: string; tool: string; detail: string; body: string[] }
  | { kind: 'result'; id: string; failed: boolean; summary: string; body: string[] };

export type Mode = 'observe' | 'moderate' | 'bypass';
export type DecisionStatus = 'deciding' | 'needsYou';
export type AnswerHow = 'suggestion' | 'text' | 'keys' | 'allow' | 'dismiss';
export type ActivityBy = 'octo' | 'rule' | 'you';
export type ActivityOutcome = 'answered' | 'denied' | 'escalated' | 'failed';
export type OctoRole = 'you' | 'octo';

export type ProjectMode = {
  path: string;
  mode: Mode;
};

export type Engine = {
  kind: string;
  installed: boolean;
};

export type Suggestion = {
  keys: string[] | null;
  text: string | null;
  summary: string;
  reason: string;
};

export type AskedTool = {
  name: string;
  detail: string;
};

export type Decision = {
  id: number;
  agent: number;
  name: string;
  kind: string;
  session: string;
  project: string;
  since: number;
  status: DecisionStatus;
  question: string;
  context: string[];
  tool: AskedTool | null;
  suggestion: Suggestion | null;
  tag: string | null;
  problem: string | null;
};

export type ActivityEntry = {
  seq: number;
  at: number;
  agent: number;
  name: string;
  kind: string;
  project: string;
  question: string;
  summary: string;
  by: ActivityBy;
  outcome: ActivityOutcome;
  mode: Mode;
};

export type OctoChatEntry = {
  at: number;
  role: OctoRole;
  text: string;
  actions: string[];
  failed: boolean;
};

export type HandledCount = {
  path: string;
  count: number;
};

export type OrchestratorView = {
  online: boolean;
  enabled: boolean;
  mode: Mode;
  projects: ProjectMode[];
  primary: string | null;
  secondary: string | null;
  primaryModel: string | null;
  secondaryModel: string | null;
  engines: Engine[];
  decisions: Decision[];
  handled: ActivityEntry[];
  handledToday: number;
  handledByProject: HandledCount[];
  chat: OctoChatEntry[];
  thinking: boolean;
  problem: string | null;
};

export type Welcome = { type: 'welcome'; host: string; version: string };
export type StateMsg = { type: 'state' } & Overview;
export type UsageMsg = { type: 'usage'; entries: UsageEntry[] };
export type ScreenMsg = { type: 'screen' } & Screen;
export type NoticeMsg = { type: 'notice'; text: string; tone: Tone };
export type FoldersMsg = { type: 'folders'; seq: number; path: string; parent: string | null; dirs: Dir[]; repo: string | null; missing: boolean; truncated: boolean };
export type FolderSearchMsg = { type: 'folderSearch'; seq: number; root: string; query: string; hits: FolderHit[]; truncated: boolean };
export type DiffMsg = { type: 'diff'; agent: number; branch: string; text: string; truncated: boolean };
export type PeekMsg = { type: 'peek'; previews: Preview[] };
export type ChatMsg = { type: 'chat'; agent: number; session: string | null; next: number; reset: boolean; items: ChatItem[] };
export type OrchestratorMsg = { type: 'orchestrator' } & OrchestratorView;
export type OrchestratorInstructionsMsg = { type: 'orchestratorInstructions'; project: string | null; text: string; path: string };
export type OrchestratorActivityMsg = { type: 'orchestratorActivity'; items: ActivityEntry[]; next: number | null };
export type AttachedMsg = { type: 'attached'; seq: number; path: string | null; problem: string | null };

export type BridgeMsg =
  | Welcome
  | StateMsg
  | UsageMsg
  | ScreenMsg
  | NoticeMsg
  | FoldersMsg
  | FolderSearchMsg
  | DiffMsg
  | PeekMsg
  | ChatMsg
  | OrchestratorMsg
  | OrchestratorInstructionsMsg
  | OrchestratorActivityMsg
  | AttachedMsg;

export type PushTo = { token: string; device: string };

export type DeviceMsg = { type: 'device'; model: string | null; osVersion: string | null; standalone: boolean; client: string | null; push?: PushTo | null };

export type StartMsg = { type: 'start'; kind: AgentKind; session: string; cwd: string; task: string; worktree: boolean; bypass: boolean; attachments?: string[] };
export type AttachMsg = { type: 'attach'; seq: number; name: string; part: number; last: boolean; data: string };
export type OrchestratorSetMsg = {
  type: 'orchestratorSet';
  enabled?: boolean;
  mode?: Mode;
  project?: { path: string; mode: Mode | null };
  primary?: string | null;
  secondary?: string | null;
  primaryModel?: string | null;
  secondaryModel?: string | null;
};
export type OrchestratorAnswerMsg = { type: 'orchestratorAnswer'; decision: number; how: AnswerHow; text?: string; keys?: string[]; remember?: string };
export type OrchestratorChatMsg = { type: 'orchestratorChat'; text: string };
export type OrchestratorAskInstructionsMsg = { type: 'orchestratorInstructions'; project: string | null };
export type OrchestratorSaveInstructionsMsg = { type: 'orchestratorSaveInstructions'; project: string | null; text: string };
export type OrchestratorAskActivityMsg = { type: 'orchestratorActivity'; before: number | null };

export type PhoneMsg =
  | DeviceMsg
  | { type: 'open'; agent: number; rows: number; cols: number }
  | { type: 'close' }
  | { type: 'size'; rows: number; cols: number }
  | { type: 'keys'; data: string }
  | { type: 'send'; text: string }
  | { type: 'scroll'; lines: number }
  | { type: 'stop'; agent: number }
  | { type: 'remove'; agent: number }
  | { type: 'rename'; agent: number; name: string }
  | StartMsg
  | { type: 'session'; name: string }
  | { type: 'recover'; session: string }
  | { type: 'folders'; path: string; seq: number }
  | { type: 'folderSearch'; root: string; query: string; seq: number }
  | { type: 'diff'; agent: number }
  | { type: 'merge'; agent: number }
  | { type: 'peek'; agents: number[]; rows: number }
  | { type: 'chat'; agent: number; session: string | null; from: number }
  | AttachMsg
  | { type: 'orchestrator' }
  | OrchestratorSetMsg
  | OrchestratorAnswerMsg
  | OrchestratorChatMsg
  | { type: 'orchestratorClearChat' }
  | OrchestratorAskInstructionsMsg
  | OrchestratorSaveInstructionsMsg
  | OrchestratorAskActivityMsg;
