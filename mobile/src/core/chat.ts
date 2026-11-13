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

export type Piece = {
  code: boolean;
  text: string;
};

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

export function pieces(text: string): Piece[] {
  return text
    .split(FENCE)
    .map((part, index) => {
      const code = index % 2 === 1;
      return { code, text: code ? part.replace(/^[\w+-]*\n/, '').replace(/\n$/, '') : part.trim() };
    })
    .filter(piece => piece.text !== '');
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
