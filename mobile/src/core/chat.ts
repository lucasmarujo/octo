import type { ChatItem } from './protocol';

export type Outcome = {
  failed: boolean;
  summary: string;
  body: string[];
};

export type Step = {
  id: string;
  tool: string;
  detail: string;
  body: string[];
  outcome: Outcome | null;
};

export type Block = { type: 'user'; key: string; text: string } | { type: 'text'; key: string; text: string } | { type: 'steps'; key: string; steps: Step[] };

export type Note = {
  text: string;
  added: number;
  removed: number;
};

export type Table = {
  head: string[];
  rows: string[][];
};

export type Piece = { kind: 'text'; text: string } | { kind: 'code'; text: string } | { kind: 'table'; table: Table };

export type Span = {
  text: string;
  bold: boolean;
  code: boolean;
};

export const MAX_BLOCKS = 400;
const CHAT_SINCE = [0, 2, 36];
const KEY_PREFIX = 'b';
const ADDED = '+';
const REMOVED = '-';
const FENCE = '```';
const PATH_SEPARATOR = /[\\/]/;
const DASHES = /--/g;
const WORD_JOINER = '\u2060';
const TITLES: Readonly<Record<string, string>> = {
  Read: 'Read',
  Edit: 'Edited',
  MultiEdit: 'Edited',
  NotebookEdit: 'Edited',
  Write: 'Wrote',
  Bash: 'Ran',
  Grep: 'Searched',
  Glob: 'Searched',
  Task: 'Asked a subagent',
  Agent: 'Asked a subagent',
  TodoWrite: 'Updated the plan',
  WebFetch: 'Fetched',
  WebSearch: 'Searched the web',
  Skill: 'Used a skill',
};
const FILE_TOOLS = ['Read', 'Edit', 'MultiEdit', 'NotebookEdit', 'Write'];
const CHANGE_TOOLS = ['Edit', 'MultiEdit', 'NotebookEdit', 'Write'];

export function atLeast(version: string | null, since: readonly number[]): boolean {
  if (!version) return false;
  const parts = version.split('.').map(part => Number.parseInt(part, 10));
  for (let index = 0; index < since.length; index += 1) {
    const part = Number.isFinite(parts[index]) ? parts[index] : 0;
    if (part !== since[index]) return part > since[index];
  }
  return true;
}

export function chatCapable(version: string | null): boolean {
  return atLeast(version, CHAT_SINCE);
}

function nextKey(blocks: Block[]): number {
  const last = blocks[blocks.length - 1];
  return last ? Number(last.key.slice(KEY_PREFIX.length)) + 1 : 0;
}

function settle(blocks: Block[], id: string, outcome: Outcome): void {
  for (let at = blocks.length - 1; at >= 0; at -= 1) {
    const block = blocks[at];
    if (block.type !== 'steps') continue;
    const index = block.steps.findIndex(step => step.id === id);
    if (index < 0) continue;
    const steps = block.steps.slice();
    steps[index] = { ...steps[index], outcome };
    blocks[at] = { ...block, steps };
    return;
  }
}

export function fold(known: Block[], items: ChatItem[]): Block[] {
  if (!items.length) return known;
  const blocks = known.slice();
  let key = nextKey(blocks);
  for (const item of items) {
    if (item.kind === 'result') {
      settle(blocks, item.id, { failed: item.failed, summary: item.summary, body: item.body });
      continue;
    }
    if (item.kind === 'step') {
      const step: Step = { id: item.id, tool: item.tool, detail: item.detail, body: item.body, outcome: null };
      const last = blocks[blocks.length - 1];
      if (last && last.type === 'steps') blocks[blocks.length - 1] = { ...last, steps: [...last.steps, step] };
      else {
        blocks.push({ type: 'steps', key: `${KEY_PREFIX}${key}`, steps: [step] });
        key += 1;
      }
      continue;
    }
    blocks.push({ type: item.kind, key: `${KEY_PREFIX}${key}`, text: item.text });
    key += 1;
  }
  return blocks.length > MAX_BLOCKS ? blocks.slice(blocks.length - MAX_BLOCKS) : blocks;
}

export function stepTitle(step: Step): string {
  const verb = TITLES[step.tool] || step.tool;
  if (!step.detail) return verb;
  const detail = FILE_TOOLS.includes(step.tool) ? step.detail.split(PATH_SEPARATOR).pop() || step.detail : step.detail;
  return `${verb} ${detail}`;
}

export function unjoined(text: string): string {
  return text.replace(DASHES, `-${WORD_JOINER}-`);
}

export function stepNote(step: Step): Note {
  if (CHANGE_TOOLS.includes(step.tool)) {
    const added = step.body.filter(line => line.startsWith(ADDED)).length;
    const removed = step.body.filter(line => line.startsWith(REMOVED)).length;
    return { text: '', added, removed };
  }
  if (step.outcome && step.outcome.failed) return { text: 'failed', added: 0, removed: 0 };
  return { text: '', added: 0, removed: 0 };
}

export function lineKind(line: string): 'add' | 'del' | 'ctx' {
  if (line.startsWith(ADDED)) return 'add';
  return line.startsWith(REMOVED) ? 'del' : 'ctx';
}

const TABLE_RULE = /^\s*\|?\s*:?-+:?\s*(\|\s*:?-+:?\s*)*\|?\s*$/;
const ESCAPED_PIPE = /\\\|/g;
const PIPE = '|';
const HELD_PIPE = '\u0000';
const HELD_PIPES = /\u0000/g;
const MARKS = /\*\*|`/g;
export const MAX_TABLE_ROWS = 200;
export const COLUMN_CHAR_PX = 7.5;
export const COLUMN_PADDING_PX = 20;
export const MIN_COLUMN_PX = 64;
export const MAX_COLUMN_PX = 240;

function cells(line: string): string[] {
  let row = line.trim().replace(ESCAPED_PIPE, HELD_PIPE);
  if (row.startsWith(PIPE)) row = row.slice(1);
  if (row.endsWith(PIPE)) row = row.slice(0, -1);
  return row.split(PIPE).map(cell => cell.trim().replace(HELD_PIPES, PIPE));
}

function fit(row: string[], width: number): string[] {
  return Array.from({ length: width }, (_, at) => row[at] ?? '');
}

function startsTable(lines: string[], at: number): boolean {
  const next = lines[at + 1];
  return lines[at].includes(PIPE) && next !== undefined && next.includes(PIPE) && TABLE_RULE.test(next) && cells(next).length === cells(lines[at]).length;
}

function prose(text: string): Piece[] {
  const lines = text.split('\n');
  const found: Piece[] = [];
  let kept: string[] = [];
  const flush = () => {
    const said = kept.join('\n').trim();
    if (said) found.push({ kind: 'text', text: said });
    kept = [];
  };
  let at = 0;
  while (at < lines.length) {
    if (!startsTable(lines, at)) {
      kept.push(lines[at]);
      at += 1;
      continue;
    }
    flush();
    const head = cells(lines[at]);
    const rows: string[][] = [];
    at += 2;
    while (at < lines.length && lines[at].trim() && lines[at].includes(PIPE)) {
      if (rows.length < MAX_TABLE_ROWS) rows.push(fit(cells(lines[at]), head.length));
      at += 1;
    }
    found.push({ kind: 'table', table: { head, rows } });
  }
  flush();
  return found;
}

export function pieces(text: string): Piece[] {
  return text.split(FENCE).flatMap((part, index): Piece[] => {
    if (index % 2 === 0) return prose(part);
    const code = part.replace(/^[\w+-]*\n/, '').replace(/\n$/, '');
    return code === '' ? [] : [{ kind: 'code', text: code }];
  });
}

export function columnWidths(table: Table): number[] {
  return table.head.map((title, column) => {
    const longest = [title, ...table.rows.map(row => row[column])].reduce((most, cell) => Math.max(most, cell.replace(MARKS, '').length), 0);
    return Math.min(MAX_COLUMN_PX, Math.max(MIN_COLUMN_PX, Math.ceil(longest * COLUMN_CHAR_PX) + COLUMN_PADDING_PX));
  });
}

const HEADING = /^#{1,6}\s+/;
const BULLET = /^(\s*)[-*]\s+/;
const INLINE = /(\*\*[^*\n]+\*\*|`[^`\n]+`)/;
const BOLD_MARK = '**';
const CODE_MARK = '`';

function inline(line: string, bold: boolean): Span[] {
  return line
    .split(INLINE)
    .filter(part => part !== '')
    .map(part => {
      if (part.startsWith(BOLD_MARK) && part.endsWith(BOLD_MARK) && part.length > BOLD_MARK.length * 2) return { text: part.slice(BOLD_MARK.length, -BOLD_MARK.length), bold: true, code: false };
      if (part.startsWith(CODE_MARK) && part.endsWith(CODE_MARK) && part.length > 2) return { text: part.slice(1, -1), bold, code: true };
      return { text: part, bold, code: false };
    });
}

export function spans(text: string): Span[] {
  const lines = text.split('\n');
  return lines.flatMap((line, index) => {
    const heading = HEADING.test(line);
    const shown = heading ? line.replace(HEADING, '') : line.replace(BULLET, '$1• ');
    const parts = inline(shown, heading);
    const end = index < lines.length - 1 ? '\n' : '';
    if (!parts.length) return end ? [{ text: end, bold: false, code: false }] : [];
    parts[parts.length - 1] = { ...parts[parts.length - 1], text: parts[parts.length - 1].text + end };
    return parts;
  });
}

export function running(blocks: Block[]): Step | null {
  const last = blocks[blocks.length - 1];
  if (!last || last.type !== 'steps') return null;
  const step = last.steps[last.steps.length - 1];
  return step.outcome ? null : step;
}
