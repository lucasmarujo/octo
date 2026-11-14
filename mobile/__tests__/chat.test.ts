import type { Block } from '../src/core/chat';
import { chatCapable, COLUMN_CHAR_PX, COLUMN_PADDING_PX, columnWidths, fold, lineKind, MAX_BLOCKS, MAX_COLUMN_PX, MAX_TABLE_ROWS, MIN_COLUMN_PX, pieces, running, spans, stepNote, stepTitle, unjoined } from '../src/core/chat';
import type { DevicePrefs } from '../src/core/devices';
import type { Identity, Transport } from '../src/core/link';
import { DeviceLink } from '../src/core/link';
import type { AgentView, BridgeMsg, ChatItem, PhoneMsg } from '../src/core/protocol';
import { CHAT_EVERY_MS, Workspace } from '../src/core/workspace';
import { hiddenBy, overlap } from '../src/ui/keyboard';

const IDENTITY: Identity = { agent: 'Octo/1.0.0', device: { type: 'device', model: 'Pixel 9', osVersion: '16', standalone: true, client: 'phone-1' } };
const PREFS: DevicePrefs = { who: null, worktree: false, bypass: false };
const ASKED: ChatItem = { kind: 'user', text: 'Fix the flaky test' };
const READ: ChatItem = { kind: 'step', id: 't1', tool: 'Read', detail: 'tests/holder_e2e.rs', body: [] };
const READ_DONE: ChatItem = { kind: 'result', id: 't1', failed: false, summary: '1 use std::fs;', body: ['1 use std::fs;'] };
const SAID: ChatItem = { kind: 'text', text: 'I will wait for the file.' };
const EDIT: ChatItem = { kind: 'step', id: 't2', tool: 'Edit', detail: 'C:\\repo\\tests\\holder_e2e.rs', body: ['- sleep(500);', '- read()', '+ wait_for(&path)'] };
const RUN: ChatItem = { kind: 'step', id: 't3', tool: 'Bash', detail: 'cargo test --test holder_e2e', body: ['cargo test --test holder_e2e'] };

function agent(part: Partial<AgentView>): AgentView {
  return { id: 7, session: 'main', name: 'holder-e2e-mac', kind: 'claude', state: 'working', starting: false, activity: '', task: '', cwd: '/w', project: 'octo', color: '#35fddc', startedAt: 1, stateSince: 1, turnStarted: 1, lastTurn: null, branch: null, exit: null, ...part };
}

function setup(version: string | null) {
  const sent: PhoneMsg[] = [];
  let session = '';
  const wire: Transport = {
    open: id => {
      session = id;
    },
    send: (_id, text) => sent.push(JSON.parse(text)),
    queued: () => 0,
    close: () => {},
  };
  const link = new DeviceLink('home', 'https://h', 'A'.repeat(43), wire, IDENTITY);
  link.connect();
  link.onReady(session);
  const hear = (message: BridgeMsg) => link.onMessage(session, JSON.stringify(message));
  if (version) hear({ type: 'welcome', host: 'lucas-desktop', version });
  hear({ type: 'state', host: 'lucas-desktop', now: Date.now(), session: 'main', sessions: [{ name: 'main', agents: 1 }], agents: [agent({})], whereDir: '/w', dirs: {}, worktree: false, kinds: ['claude'] });
  const workspace = new Workspace(link, { toast: jest.fn(), buzz: jest.fn(), diff: jest.fn(), prefs: () => PREFS, reduced: () => true, open: jest.fn() });
  sent.length = 0;
  const asked = () => sent.filter(message => message.type === 'chat');
  return { workspace, sent, hear, asked };
}

describe('the conversation of an agent', () => {
  it('reads as what was asked, what was said and the steps in between, grouped in one card', () => {
    const blocks = fold([], [ASKED, READ, READ_DONE, SAID, EDIT, RUN]);
    expect(blocks.map(block => block.type)).toEqual(['user', 'steps', 'text', 'steps']);
    expect(new Set(blocks.map(block => block.key)).size).toBe(4);
    const [, first, , second] = blocks;
    expect(first).toMatchObject({ steps: [{ id: 't1', outcome: { failed: false, summary: '1 use std::fs;' } }] });
    expect(second).toMatchObject({ steps: [{ id: 't2', outcome: null }, { id: 't3', outcome: null }] });
  });

  it('settles a step when its result comes in a later page, without touching the page before', () => {
    const before = fold([], [ASKED, EDIT, RUN]);
    const after = fold(before, [{ kind: 'result', id: 't2', failed: false, summary: 'updated', body: [] }, { kind: 'result', id: 't3', failed: true, summary: 'error: test failed', body: ['error: test failed'] }, SAID]);
    expect(before[1]).toMatchObject({ steps: [{ outcome: null }, { outcome: null }] });
    expect(after[1]).toMatchObject({ steps: [{ outcome: { failed: false } }, { outcome: { failed: true, summary: 'error: test failed' } }] });
    expect(after.map(block => block.key)).toEqual(['b0', 'b1', 'b2']);
    expect(fold(after, [])).toBe(after);
    expect(fold(after, [{ kind: 'result', id: 'unknown', failed: false, summary: '', body: [] }])).toEqual(after);
  });

  it('keeps only the end of a very long conversation, with keys that never repeat', () => {
    const many: ChatItem[] = Array.from({ length: MAX_BLOCKS + 50 }, (_, index) => ({ kind: 'text', text: `message ${index}` }));
    const blocks = fold(fold([], many), [SAID]);
    expect(blocks).toHaveLength(MAX_BLOCKS);
    expect(new Set(blocks.map(block => block.key)).size).toBe(MAX_BLOCKS);
    expect(blocks[blocks.length - 1]).toMatchObject({ type: 'text', text: 'I will wait for the file.' });
  });

  it('names each step by what the agent did, with the file and not its whole path', () => {
    const [, steps] = fold([], [ASKED, READ, EDIT, RUN, { kind: 'step', id: 't4', tool: 'TodoWrite', detail: '', body: [] }, { kind: 'step', id: 't5', tool: 'mcp__paper__get_jsx', detail: '', body: [] }]) as [Block, Extract<Block, { type: 'steps' }>];
    expect(steps.steps.map(stepTitle)).toEqual(['Read holder_e2e.rs', 'Edited holder_e2e.rs', 'Ran cargo test --test holder_e2e', 'Updated the plan', 'mcp__paper__get_jsx']);
  });

  it('keeps the two dashes of a flag apart, which the font of the app would join into one', () => {
    expect(unjoined('cargo test --test holder_e2e')).toBe('cargo test -\u2060-test holder_e2e');
    expect(unjoined('ls -la')).toBe('ls -la');
  });

  it('counts the lines an edit added and removed, and says when a step failed', () => {
    const [steps] = fold([], [EDIT, RUN, { kind: 'result', id: 't3', failed: true, summary: 'boom', body: [] }]) as [Extract<Block, { type: 'steps' }>];
    expect(stepNote(steps.steps[0])).toEqual({ text: '', added: 1, removed: 2 });
    expect(stepNote(steps.steps[1])).toEqual({ text: 'failed', added: 0, removed: 0 });
    expect(['+ a', '- b', 'c'].map(lineKind)).toEqual(['add', 'del', 'ctx']);
  });

  it('knows which step is still running: the last one, while it has no result', () => {
    expect(running(fold([], [ASKED, READ, READ_DONE, RUN]))).toMatchObject({ id: 't3' });
    expect(running(fold([], [ASKED, READ, READ_DONE]))).toBeNull();
    expect(running(fold([], [RUN, SAID]))).toBeNull();
    expect(running([])).toBeNull();
  });

  it('sets code apart from the text around it', () => {
    expect(pieces('Run this:\n```bash\ncargo test\n```\nand tell me.')).toEqual([
      { kind: 'text', text: 'Run this:' },
      { kind: 'code', text: 'cargo test' },
      { kind: 'text', text: 'and tell me.' },
    ]);
    expect(pieces('just text')).toEqual([{ kind: 'text', text: 'just text' }]);
    expect(pieces('')).toEqual([]);
  });

  it('turns a markdown table into rows and columns instead of a wall of pipes', () => {
    const said = [
      '**Modified Files**',
      '',
      '| Arquivo | Seção | Motivo |',
      '|---|---|---|',
      '| `infra/compose/docker-compose.stack.yml` | Serviço `autoheal` | Autoheal por ambiente |',
      '| README.md | Deploy |',
      '| a \\| b | c | d | extra |',
      '',
      'Commit 986948d.',
    ].join('\n');
    expect(pieces(said)).toEqual([
      { kind: 'text', text: '**Modified Files**' },
      {
        kind: 'table',
        table: {
          head: ['Arquivo', 'Seção', 'Motivo'],
          rows: [
            ['`infra/compose/docker-compose.stack.yml`', 'Serviço `autoheal`', 'Autoheal por ambiente'],
            ['README.md', 'Deploy', ''],
            ['a | b', 'c', 'd'],
          ],
        },
      },
      { kind: 'text', text: 'Commit 986948d.' },
    ]);
  });

  it('reads aligned tables, tables without outer pipes and tables right after the text', () => {
    expect(pieces('Results:\n| Test | Time |\n|:---|---:|\n| unit | 3s |')).toEqual([
      { kind: 'text', text: 'Results:' },
      { kind: 'table', table: { head: ['Test', 'Time'], rows: [['unit', '3s']] } },
    ]);
    expect(pieces('a | b\n--- | ---\n1 | 2\n\nafter')).toEqual([
      { kind: 'table', table: { head: ['a', 'b'], rows: [['1', '2']] } },
      { kind: 'text', text: 'after' },
    ]);
    expect(pieces('| only | head |\n|---|---|')).toEqual([{ kind: 'table', table: { head: ['only', 'head'], rows: [] } }]);
  });

  it('leaves pipes that are not a table alone', () => {
    expect(pieces('use a | b to pipe\nand c | d too')).toEqual([{ kind: 'text', text: 'use a | b to pipe\nand c | d too' }]);
    expect(pieces('| a | b |\n|---|')).toEqual([{ kind: 'text', text: '| a | b |\n|---|' }]);
    expect(pieces('Title | x\n---')).toEqual([{ kind: 'text', text: 'Title | x\n---' }]);
    expect(pieces('```\n| a | b |\n|---|---|\n```')).toEqual([{ kind: 'code', text: '| a | b |\n|---|---|' }]);
  });

  it('keeps a long table bounded', () => {
    const rows = Array.from({ length: MAX_TABLE_ROWS + 50 }, (_, at) => `| ${at} | x |`);
    const [piece] = pieces(['| n | v |', '|---|---|', ...rows, '', 'end'].join('\n'));
    expect(piece.kind === 'table' && piece.table.rows.length).toBe(MAX_TABLE_ROWS);
  });

  it('sizes each column by its longest cell, within limits', () => {
    const widths = columnWidths({ head: ['a', 'Motivo'], rows: [['**ok**', 'x'.repeat(200)]] });
    expect(widths[0]).toBe(MIN_COLUMN_PX);
    expect(widths[1]).toBe(MAX_COLUMN_PX);
    const middle = columnWidths({ head: ['`twelve chars`'], rows: [] });
    expect(middle[0]).toBe(Math.ceil(12 * COLUMN_CHAR_PX) + COLUMN_PADDING_PX);
  });

  it('reads titles, bold words, inline code and lists the way the agent meant them', () => {
    expect(spans('## Summary')).toEqual([{ text: 'Summary', bold: true, code: false }]);
    expect(spans('Run **cargo test** in `desktop/`.')).toEqual([
      { text: 'Run ', bold: false, code: false },
      { text: 'cargo test', bold: true, code: false },
      { text: ' in ', bold: false, code: false },
      { text: 'desktop/', bold: false, code: true },
      { text: '.', bold: false, code: false },
    ]);
    expect(spans('- one\n- two').map(span => span.text).join('')).toBe('• one\n• two');
    expect(spans('a\n\nb').map(span => span.text).join('')).toBe('a\n\nb');
    expect(spans('2 * 3 * 4')).toEqual([{ text: '2 * 3 * 4', bold: false, code: false }]);
  });
});

describe('asking the computer for the conversation', () => {
  beforeEach(() => jest.useFakeTimers());
  afterEach(() => jest.useRealTimers());

  it('only happens with an Octo that knows how to answer, so an older one never complains', () => {
    expect(['0.2.36', '0.2.40', '0.3.0', '1.0.0'].map(chatCapable)).toEqual([true, true, true, true]);
    expect(['0.2.35', '0.1.99', '', 'dev'].map(chatCapable)).toEqual([false, false, false, false]);
    expect(chatCapable(null)).toBe(false);
    const old = setup('0.2.35');
    old.workspace.openAgent(7);
    jest.advanceTimersByTime(CHAT_EVERY_MS * 3);
    expect(old.asked()).toEqual([]);
    old.workspace.dispose();
  });

  it('asks when the agent opens, then only for what is new, and stops when the agent is left', () => {
    const { workspace, hear, asked } = setup('0.2.36');
    workspace.openAgent(7);
    expect(asked()).toEqual([{ type: 'chat', agent: 7, session: null, from: 0 }]);
    hear({ type: 'chat', agent: 7, session: 's1', next: 120, reset: true, items: [ASKED, READ] });
    expect(workspace.chat.get()).toMatchObject({ session: 's1', next: 120, heard: true });
    expect(workspace.chat.get()!.blocks.map(block => block.type)).toEqual(['user', 'steps']);
    jest.advanceTimersByTime(CHAT_EVERY_MS);
    expect(asked()[1]).toEqual({ type: 'chat', agent: 7, session: 's1', from: 120 });
    hear({ type: 'chat', agent: 7, session: 's1', next: 200, reset: false, items: [READ_DONE, SAID] });
    expect(workspace.chat.get()!.blocks).toMatchObject([{ type: 'user' }, { steps: [{ outcome: { failed: false } }] }, { type: 'text' }]);
    const same = workspace.chat.get();
    hear({ type: 'chat', agent: 7, session: 's1', next: 200, reset: false, items: [] });
    expect(workspace.chat.get()).toBe(same);
    workspace.leaveAgent();
    const count = asked().length;
    jest.advanceTimersByTime(CHAT_EVERY_MS * 3);
    expect(asked()).toHaveLength(count);
    expect(workspace.chat.get()).toBeNull();
  });

  it('starts over when the agent moved to another conversation, and ignores the answer about another agent', () => {
    const { workspace, hear } = setup('0.2.36');
    workspace.openAgent(7);
    hear({ type: 'chat', agent: 7, session: 's1', next: 120, reset: true, items: [ASKED, SAID] });
    hear({ type: 'chat', agent: 9, session: 's9', next: 50, reset: true, items: [SAID] });
    expect(workspace.chat.get()).toMatchObject({ session: 's1', next: 120 });
    hear({ type: 'chat', agent: 7, session: 's2', next: 40, reset: true, items: [SAID] });
    expect(workspace.chat.get()).toMatchObject({ session: 's2', next: 40 });
    expect(workspace.chat.get()!.blocks).toHaveLength(1);
    workspace.dispose();
  });

  it('opens on the conversation, shows the raw terminal on demand and interrupts with escape', () => {
    const { workspace, sent } = setup('0.2.36');
    workspace.openAgent(7);
    expect(workspace.store.get().raw).toBe(false);
    workspace.showRaw(true);
    expect(workspace.store.get().raw).toBe(true);
    workspace.interrupt();
    expect(sent[sent.length - 1]).toEqual({ type: 'keys', data: '\x1b' });
    workspace.openAgent(7);
    expect(workspace.store.get().raw).toBe(false);
    workspace.dispose();
  });
});

describe('the room the keyboard takes', () => {
  it('is what really sits over the app, whatever the system reports as its height', () => {
    expect(overlap(844, 508)).toBe(336);
    expect(overlap(844, 844)).toBe(0);
    expect(overlap(844, 0)).toBe(0);
    expect(hiddenBy({ height: 291, top: 508 }, 844, 34, 'ios')).toBe(336);
    expect(hiddenBy({ height: 290, top: 0 }, 844, 48, 'android')).toBe(338);
    expect(hiddenBy({ height: 290, top: 470 }, 844, 48, 'android')).toBe(374);
    expect(hiddenBy(null, 844, 48, 'android')).toBe(0);
  });
});
