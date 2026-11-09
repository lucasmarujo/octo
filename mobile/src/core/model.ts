import type { AgentKind, AgentState, Run } from './protocol';

export type Column = 'needs' | 'working' | 'done' | 'idle';
export type Mood = 'needs' | 'working' | 'done' | 'lost' | 'idle';
export type ThemeChoice = 'dark' | 'gray' | 'light' | 'system';
export type ThemeName = 'dark' | 'gray' | 'light';
export type UsageTone = 'green' | 'yellow' | 'red';

export type ColumnDef = {
  key: Column;
  title: string;
  empty: string;
};

export type Choice = {
  key: string;
  label: string;
  danger: boolean;
};

export type Asked = {
  question: string;
  context: string[];
  options: Choice[];
};

export type DiffLine = {
  kind: 'add' | 'del' | 'ctx';
  number: number | null;
  text: string;
};

export type DiffHunk = {
  title: string;
  lines: DiffLine[];
};

export type DiffFile = {
  name: string;
  insertions: number;
  deletions: number;
  hunks: DiffHunk[];
};

export type DiffTotals = {
  files: number;
  insertions: number;
  deletions: number;
};

type Stateful = {
  state: AgentState;
  starting: boolean;
};

type Model = {
  SPINNER: string[];
  USAGE_WARN: number;
  USAGE_ALERT: number;
  THEMES: ThemeChoice[];
  THEME_INK: Record<ThemeName, string>;
  COLUMNS: ColumnDef[];
  duration(ms: number): string;
  column(agent: Stateful): Column;
  moodOf(list: Stateful[]): Mood;
  usageTone(used: number): UsageTone;
  arc(used: number, radius: number): string;
  resolveTheme(choice: string | null, prefersLight: boolean): ThemeName;
  kindMark(kind: AgentKind | string): string;
  shortPath(path: string): string;
  crumbs(path: string): string[];
  runsText(runs: Run[]): string;
  choices(lines: string[]): Asked | null;
  diffFiles(text: string): DiffFile[];
  diffTotals(files: DiffFile[]): DiffTotals;
  shortModel(id: string | null | undefined): string;
};

const model: Model = require('../../../src/pwa/web/model.js');

export default model;
