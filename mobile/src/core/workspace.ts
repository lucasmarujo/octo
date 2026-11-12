import { alive, waiting } from './agents';
import type { Block } from './chat';
import { chatCapable, fold } from './chat';
import type { DevicePrefs } from './devices';
import type { Buzz } from './haptics';
import type { DeviceLink } from './link';
import type { Asked, Choice } from './model';
import model from './model';
import type { AgentKind, AgentView, BridgeMsg, ChatMsg, DiffMsg, FolderSearchMsg, FoldersMsg, Overview, PhoneMsg, Run, ScreenMsg, Tone } from './protocol';
import { Store } from './store';

export const TERM_FONT = 12;
export const TERM_MIN = 6;
export const TERM_MAX = 15;
export const TERM_LINE = 1.2;
export const TERM_PAD = 6;
export const MIN_ROWS = 8;
export const MIN_COLS = 20;
const PEEK_ROWS = 18;
const PENDING_MS = 6000;
const ANSWER_MS = 240;
const SCROLL_EVERY_MS = 40;
export const CHAT_EVERY_MS = 1500;
const INTERRUPT = '';
const ENTER = '\r';
const FALLBACK_DIR = '~';
const FALLBACK_KIND: AgentKind = 'terminal';
const PREFERRED_KIND: AgentKind = 'claude';

export type TermSize = {
  rows: number;
  cols: number;
};

export type ScreenState = {
  rows: Run[][];
  cols: number;
  cursor: [row: number, col: number] | null;
  scroll: number;
};

export type ChatState = {
  agent: number;
  session: string | null;
  next: number;
  blocks: Block[];
  heard: boolean;
};

export type Approval = {
  asked: Asked;
  sig: string;
  answered: boolean;
};

export type WorkspaceState = {
  view: 'overview' | 'agent';
  filter: string | null;
  agent: number | null;
  where: string | null;
  previews: ReadonlyMap<number, Asked>;
  approval: Approval | null;
  raw: boolean;
  entered: number;
};

export type ToastAsk = {
  title?: string;
  mark?: { text: string; tile: 'idle' | 'working' };
};

export type Effects = {
  toast(text: string, tone: Tone, ask?: ToastAsk): void;
  buzz(kind: Buzz): void;
  diff(message: DiffMsg): void;
  prefs(): DevicePrefs;
  reduced(): boolean;
};

export type FolderNews = FoldersMsg | FolderSearchMsg;

type Pending = {
  agent: number;
  key: string;
  label: string;
  at: number;
};

const NO_RUNS: Run[] = [];

export class Workspace {
  readonly store: Store<WorkspaceState>;
  readonly screen = new Store<ScreenState | null>(null);
  readonly chat = new Store<ChatState | null>(null);
  private chatTimer: ReturnType<typeof setInterval> | null = null;
  private wanted: TermSize | null = null;
  private opened = false;
  private peekKey = '';
  private pending: Pending | null = null;
  private approvalSig = '';
  private answeredSig = '';
  private answerTimer: ReturnType<typeof setTimeout> | null = null;
  private scrollTimer: ReturnType<typeof setTimeout> | null = null;
  private scrollLines = 0;
  private folders: ((news: FolderNews) => void) | null = null;
  private folderSeq = 0;
  private readonly unlisten: () => void;

  constructor(readonly link: DeviceLink, private readonly effects: Effects) {
    const overview = link.store.get().overview;
    this.store = new Store<WorkspaceState>({
      view: 'overview',
      filter: overview ? overview.session : null,
      agent: null,
      where: null,
      previews: new Map(),
      approval: null,
      raw: false,
      entered: 0,
    });
    this.unlisten = link.listen(message => this.hear(message));
    this.syncPeek();
  }

  dispose(): void {
    this.unlisten();
    if (this.answerTimer) clearTimeout(this.answerTimer);
    if (this.scrollTimer) clearTimeout(this.scrollTimer);
    this.stopChat();
    if (this.store.get().view === 'agent') this.link.send({ type: 'close' });
    this.folders = null;
  }

  overview(): Overview | null {
    return this.link.store.get().overview;
  }

  agents(): AgentView[] {
    const overview = this.overview();
    return overview ? overview.agents : [];
  }

  inSession(): AgentView[] {
    const filter = this.store.get().filter;
    return this.agents().filter(agent => agent.session === filter);
  }

  findAgent(id: number | null): AgentView | null {
    return this.agents().find(agent => agent.id === id) || null;
  }

  currentWho(): AgentKind {
    const overview = this.overview();
    const kinds = overview ? overview.kinds : [];
    const chosen = this.effects.prefs().who;
    if (chosen && kinds.includes(chosen)) return chosen;
    return kinds.includes(PREFERRED_KIND) ? PREFERRED_KIND : kinds[0] || FALLBACK_KIND;
  }

  currentWhere(): string {
    const state = this.store.get();
    if (state.where) return state.where;
    const overview = this.overview();
    if (!overview) return FALLBACK_DIR;
    return (state.filter !== null && overview.dirs[state.filter]) || overview.whereDir || FALLBACK_DIR;
  }

  send(message: PhoneMsg): void {
    this.link.send(message);
  }

  pickSession(name: string): void {
    if (name === this.store.get().filter) return;
    this.store.update(state => ({ ...state, filter: name, where: null, entered: state.entered + 1 }));
    this.syncPeek();
  }

  newSession(name: string): void {
    this.link.send({ type: 'session', name });
    this.store.update(state => ({ ...state, filter: name, where: null, entered: state.entered + 1 }));
  }

  setWhere(path: string): void {
    this.store.patch({ where: path });
  }

  replay(): void {
    this.store.update(state => ({ ...state, entered: state.entered + 1 }));
  }

  openAgent(id: number): void {
    this.screen.set(null);
    this.approvalSig = '';
    this.answeredSig = '';
    this.opened = false;
    this.store.patch({ view: 'agent', agent: id, approval: null, raw: false });
    this.startChat(id);
  }

  leaveAgent(): void {
    if (this.store.get().view !== 'agent') return;
    this.link.send({ type: 'close' });
    this.screen.set(null);
    this.stopChat();
    this.opened = false;
    this.store.update(state => ({ ...state, view: 'overview', agent: null, approval: null, entered: state.entered + 1 }));
    this.syncPeek();
  }

  fit(size: TermSize): void {
    const state = this.store.get();
    if (state.view !== 'agent' || state.agent === null) return;
    const changed = !this.wanted || size.rows !== this.wanted.rows || size.cols !== this.wanted.cols;
    this.wanted = size;
    if (!this.opened) {
      this.opened = true;
      this.link.send({ type: 'open', agent: state.agent, rows: size.rows, cols: size.cols });
    } else if (changed) {
      this.link.send({ type: 'size', rows: size.rows, cols: size.cols });
    }
  }

  refit(): void {
    if (this.wanted && this.opened) this.link.send({ type: 'size', rows: this.wanted.rows, cols: this.wanted.cols });
  }

  showRaw(raw: boolean): void {
    this.store.patch({ raw });
  }

  interrupt(): void {
    this.link.send({ type: 'keys', data: INTERRUPT });
    this.effects.buzz('tap');
  }

  keys(data: string): void {
    this.link.send({ type: 'keys', data });
  }

  live(): void {
    this.link.send({ type: 'scroll', lines: 0 });
  }

  scroll(lines: number): void {
    this.scrollLines += lines;
    if (this.scrollTimer) return;
    this.scrollTimer = setTimeout(() => {
      const total = this.scrollLines;
      this.scrollLines = 0;
      this.scrollTimer = null;
      if (total) this.link.send({ type: 'scroll', lines: total });
    }, SCROLL_EVERY_MS);
  }

  answer(key: string, sig: string): void {
    this.link.send({ type: 'keys', data: key });
    this.effects.buzz('tap');
    this.answeredSig = sig;
    const approval = this.store.get().approval;
    if (approval) this.store.patch({ approval: { ...approval, answered: true } });
    if (this.answerTimer) clearTimeout(this.answerTimer);
    this.answerTimer = setTimeout(
      () => {
        this.answerTimer = null;
        if (this.answeredSig === sig) this.store.patch({ approval: null });
      },
      this.effects.reduced() ? 0 : ANSWER_MS,
    );
  }

  answerFromOverview(agent: AgentView, choice: Choice): void {
    this.pending = { agent: agent.id, key: choice.key, label: choice.label, at: Date.now() };
    this.openAgent(agent.id);
  }

  submit(text: string): boolean {
    const state = this.store.get();
    const task = text.trim();
    if (state.view === 'agent') {
      if (task) this.link.send({ type: 'send', text });
      else this.link.send({ type: 'keys', data: ENTER });
      return true;
    }
    const who = this.currentWho();
    const ai = who !== FALLBACK_KIND;
    if (!task && ai) {
      this.effects.toast('Write the task first', 'info', { mark: { text: '✎', tile: 'idle' } });
      return false;
    }
    const prefs = this.effects.prefs();
    this.link.send({
      type: 'start',
      kind: who,
      session: state.filter || '',
      cwd: this.currentWhere(),
      task,
      worktree: prefs.worktree && ai,
      bypass: prefs.bypass && ai,
    });
    return true;
  }

  watchFolders(heard: (news: FolderNews) => void): () => void {
    this.folders = heard;
    return () => {
      if (this.folders === heard) this.folders = null;
    };
  }

  browse(path: string): void {
    this.folderSeq += 1;
    this.link.send({ type: 'folders', path, seq: this.folderSeq });
  }

  search(root: string, query: string): void {
    this.folderSeq += 1;
    this.link.send({ type: 'folderSearch', root, query, seq: this.folderSeq });
  }

  private hear(message: BridgeMsg): void {
    switch (message.type) {
      case 'welcome':
        this.peekKey = '';
        this.reopen();
        this.followChat();
        break;
      case 'state':
        this.onOverview(message);
        break;
      case 'screen':
        this.applyScreen(message);
        break;
      case 'peek':
        this.onPeek(message.previews);
        break;
      case 'notice':
        this.effects.toast(message.text, message.tone);
        break;
      case 'folders':
      case 'folderSearch':
        if (this.folders && (!message.seq || message.seq === this.folderSeq)) this.folders(message);
        break;
      case 'diff':
        this.effects.diff(message);
        break;
      case 'chat':
        this.onChat(message);
        break;
      default:
        break;
    }
  }

  private startChat(agent: number): void {
    this.stopChat();
    this.chat.set({ agent, session: null, next: 0, blocks: [], heard: false });
    this.followChat();
  }

  private followChat(): void {
    if (!this.chat.get() || !chatCapable(this.link.version)) return;
    this.askChat();
    if (!this.chatTimer) this.chatTimer = setInterval(() => this.askChat(), CHAT_EVERY_MS);
  }

  private stopChat(): void {
    if (this.chatTimer) clearInterval(this.chatTimer);
    this.chatTimer = null;
    this.chat.set(null);
  }

  private askChat(): void {
    const chat = this.chat.get();
    if (!chat || !chatCapable(this.link.version) || this.link.store.get().phase !== 'live') return;
    const agent = this.findAgent(chat.agent);
    if (chat.heard && agent && !alive(agent)) return;
    this.link.send({ type: 'chat', agent: chat.agent, session: chat.session, from: chat.next });
  }

  private onChat(message: ChatMsg): void {
    const chat = this.chat.get();
    if (!chat || chat.agent !== message.agent) return;
    const fresh = message.reset || message.session !== chat.session;
    if (!fresh && message.next === chat.next && chat.heard) return;
    this.chat.set({ agent: chat.agent, session: message.session, next: message.next, blocks: fold(fresh ? [] : chat.blocks, message.items), heard: true });
  }

  private reopen(): void {
    const state = this.store.get();
    if (state.view !== 'agent' || state.agent === null || !this.wanted) return;
    this.opened = true;
    this.link.send({ type: 'open', agent: state.agent, rows: this.wanted.rows, cols: this.wanted.cols });
  }

  private onOverview(overview: Overview): void {
    const state = this.store.get();
    if (!overview.sessions.some(session => session.name === state.filter)) {
      this.store.update(current => ({ ...current, filter: overview.session, entered: current.entered + 1 }));
    }
    this.syncPeek();
    this.judge();
  }

  private onPeek(previews: { agent: number; lines: Run[][] }[]): void {
    const next = new Map(this.store.get().previews);
    for (const preview of previews) {
      const asked = model.choices(preview.lines.map(model.runsText));
      if (asked) next.set(preview.agent, asked);
      else next.delete(preview.agent);
    }
    this.store.patch({ previews: next });
  }

  private syncPeek(): void {
    const state = this.store.get();
    if (state.view !== 'overview' || !this.overview()) return;
    const asking = this.inSession().filter(waiting);
    const key = asking.map(agent => `${agent.id}:${agent.stateSince}`).join(',');
    if (key === this.peekKey) return;
    this.peekKey = key;
    const kept = new Map([...state.previews].filter(([id]) => asking.some(agent => agent.id === id)));
    if (kept.size !== state.previews.size) this.store.patch({ previews: kept });
    if (asking.length) this.link.send({ type: 'peek', agents: asking.map(agent => agent.id), rows: PEEK_ROWS });
  }

  private applyScreen(message: ScreenMsg): void {
    if (message.agent !== this.store.get().agent) return;
    const current = this.screen.get();
    const fresh = message.full || !current || current.rows.length !== message.rows;
    const rows = fresh ? Array.from({ length: message.rows }, () => NO_RUNS) : current.rows.slice();
    for (const [index, runs] of message.lines) {
      if (index < rows.length) rows[index] = runs;
    }
    this.screen.set({ rows, cols: message.cols, cursor: message.cursor, scroll: message.scroll });
    this.settlePending();
    this.judge();
  }

  private lines(): string[] {
    const screen = this.screen.get();
    return screen ? screen.rows.map(model.runsText) : [];
  }

  private judge(): void {
    const state = this.store.get();
    if (state.view !== 'agent') return;
    const agent = this.findAgent(state.agent);
    const screen = this.screen.get();
    const asked = agent && waiting(agent) && screen && !screen.scroll ? model.choices(this.lines()) : null;
    const sig = asked ? JSON.stringify(asked) : '';
    if (!asked || sig === this.answeredSig) {
      if (state.approval) this.store.patch({ approval: null });
      this.approvalSig = '';
      if (!asked) this.answeredSig = '';
      return;
    }
    if (sig === this.approvalSig) return;
    this.approvalSig = sig;
    this.store.patch({ approval: { asked, sig, answered: false } });
  }

  private settlePending(): void {
    const pending = this.pending;
    if (!pending || pending.agent !== this.store.get().agent) return;
    this.pending = null;
    if (Date.now() - pending.at > PENDING_MS) return;
    const asked = model.choices(this.lines());
    const same = asked && asked.options.find(choice => choice.key === pending.key && choice.label === pending.label);
    if (!asked || !same) {
      this.effects.toast('The question changed. Answer it on the screen.', 'warn');
      return;
    }
    this.answer(pending.key, JSON.stringify(asked));
  }
}
