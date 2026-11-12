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

export type BridgeMsg = Welcome | StateMsg | UsageMsg | ScreenMsg | NoticeMsg | FoldersMsg | FolderSearchMsg | DiffMsg | PeekMsg | ChatMsg;

export type PushTo = { token: string; device: string };

export type DeviceMsg = { type: 'device'; model: string | null; osVersion: string | null; standalone: boolean; client: string | null; push?: PushTo | null };

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
  | { type: 'start'; kind: AgentKind; session: string; cwd: string; task: string; worktree: boolean; bypass: boolean }
  | { type: 'session'; name: string }
  | { type: 'recover'; session: string }
  | { type: 'folders'; path: string; seq: number }
  | { type: 'folderSearch'; root: string; query: string; seq: number }
  | { type: 'diff'; agent: number }
  | { type: 'merge'; agent: number }
  | { type: 'peek'; agents: number[]; rows: number }
  | { type: 'chat'; agent: number; session: string | null; from: number };
