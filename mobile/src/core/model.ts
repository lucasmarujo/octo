import type { AgentKind, AgentState, Run } from './protocol';

export type Column = 'needs' | 'working' | 'done' | 'idle';
export type Mood = 'needs' | 'working' | 'done' | 'lost' | 'idle';
export type ThemeName = 'octo' | 'gray' | 'light' | 'old';
export type ThemeChoice = ThemeName | 'system';
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
  COLUMNS: ColumnDef[];
  duration(ms: number): string;
  column(agent: Stateful): Column;
  moodOf(list: Stateful[]): Mood;
  usageTone(used: number): UsageTone;
  arc(used: number, radius: number): string;
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

export const THEME_CHOICES: ThemeChoice[] = ['octo', 'gray', 'light', 'old', 'system'];
export const DEFAULT_THEME: ThemeName = 'octo';
const SYSTEM_LIGHT: ThemeName = 'light';

export function resolveTheme(choice: ThemeChoice, prefersLight: boolean): ThemeName {
  if (choice !== 'system') return choice;
  return prefersLight ? SYSTEM_LIGHT : DEFAULT_THEME;
}

export default model;
