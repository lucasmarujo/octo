import type { Attachment, Picked, Refusal } from './attach';
import { attachMessage, base64, cancelled, CHANGED, DROPPED, failed, heard, interrupted, paths, partCount, partRange, pending, progressed, removed, started, UNANSWERED, UNREADABLE, uploading, withFiles } from './attach';
import type { DeviceLink } from './link';
import type { AttachedMsg } from './protocol';
import { Store } from './store';

export type Source = {
  read(offset: number, length: number): Promise<Uint8Array>;
  close(): void;
};

export type Opener = (uri: string) => Source;

type Run = {
  stopped: boolean;
};

type Opened = {
  seq: number;
  name: string;
};

type Waiting = {
  seq: number;
  settle: (kept: boolean) => void;
};

export const HIGH_WATER = 512 * 1024;
export const ROOM_EVERY_MS = 40;
export const ANSWER_MS = 60000;
const NO_BYTES = new Uint8Array(0);

export class Uploads {
  readonly store = new Store<Attachment[]>([]);
  private keys = 0;
  private run: Run | null = null;
  private waiting: Waiting | null = null;
  private room: (() => void) | null = null;
  private opened: Opened | null = null;

  constructor(private readonly link: DeviceLink, private readonly open: Opener, private seq: number = Date.now()) {}

  add(picked: Picked[]): Refusal[] {
    const added = withFiles(this.store.get(), picked, this.keys + 1);
    this.keys += added.list.length - this.store.get().length;
    this.store.set(added.list);
    return added.refused;
  }

  remove(key: number): void {
    const file = this.store.get().find(entry => entry.key === key);
    if (file && (file.phase === 'sending' || file.phase === 'sent')) this.cancel();
    this.store.set(removed(this.store.get(), key));
  }

  clear(): void {
    this.cancel();
    this.store.set([]);
  }

  cancel(): void {
    this.stop();
    this.store.set(cancelled(this.store.get()));
  }

  reset(): void {
    this.opened = null;
    this.stop();
    this.store.set(interrupted(this.store.get(), DROPPED));
  }

  dispose(): void {
    this.stop();
  }

  hear(message: AttachedMsg): void {
    const before = this.store.get();
    this.store.set(heard(before, message));
    if (this.opened && this.opened.seq === message.seq) this.opened = null;
    if (this.room && this.store.get() !== before) this.room();
    const waiting = this.waiting;
    if (!waiting || waiting.seq !== message.seq) return;
    const file = this.store.get().find(entry => entry.seq === message.seq);
    waiting.settle(Boolean(file && file.phase === 'done'));
  }

  async send(): Promise<string[] | null> {
    if (this.run) return null;
    const run: Run = { stopped: false };
    this.run = run;
    try {
      for (const file of pending(this.store.get())) {
        if (!(await this.upload(file.key, run))) return null;
      }
      return run.stopped ? null : paths(this.store.get());
    } finally {
      if (this.run === run) this.run = null;
    }
  }

  private stop(): void {
    if (this.run) this.run.stopped = true;
    this.run = null;
    if (this.room) this.room();
    if (this.waiting) this.waiting.settle(false);
    this.release();
  }

  private release(): void {
    const opened = this.opened;
    this.opened = null;
    if (opened && this.live()) this.link.send(attachMessage(opened.seq, opened.name, 0, true, ''));
  }

  private live(): boolean {
    return this.link.store.get().phase === 'live';
  }

  private sending(key: number, seq: number, run: Run): boolean {
    const file = this.store.get().find(entry => entry.key === key);
    return !run.stopped && Boolean(file && file.seq === seq && file.phase === 'sending');
  }

  private fail(key: number, seq: number, problem: string): false {
    const file = this.store.get().find(entry => entry.key === key);
    if (file && file.seq === seq && file.phase !== 'failed') this.store.set(failed(this.store.get(), key, problem));
    if (this.opened && this.opened.seq === seq) this.release();
    return false;
  }

  private async upload(key: number, run: Run): Promise<boolean> {
    const file = this.store.get().find(entry => entry.key === key);
    if (run.stopped) return false;
    if (!file || file.phase === 'done') return true;
    this.seq += 1;
    const seq = this.seq;
    this.store.set(started(this.store.get(), key, seq));
    if (!this.live()) return this.fail(key, seq, DROPPED);
    if (file.size < 0) return this.fail(key, seq, UNREADABLE);
    let source: Source;
    try {
      source = this.open(file.uri);
    } catch {
      return this.fail(key, seq, UNREADABLE);
    }
    try {
      const count = partCount(file.size);
      for (let part = 0; part < count; part += 1) {
        await this.wait(run, HIGH_WATER);
        if (!this.sending(key, seq, run)) return false;
        const range = partRange(file.size, part);
        const last = part === count - 1;
        const bytes = range.length ? await source.read(range.offset, range.length) : NO_BYTES;
        const beyond = last ? await source.read(file.size, 1) : NO_BYTES;
        if (!this.sending(key, seq, run)) return false;
        if (bytes.length !== range.length || beyond.length) return this.fail(key, seq, CHANGED);
        if (!this.live()) return this.fail(key, seq, DROPPED);
        this.link.send(attachMessage(seq, file.name, part, last, base64(bytes)));
        this.opened = last ? null : { seq, name: file.name };
        this.store.set(progressed(this.store.get(), key, range.offset + range.length, last));
      }
    } catch {
      return run.stopped ? false : this.fail(key, seq, UNREADABLE);
    } finally {
      source.close();
    }
    await this.wait(run, 0);
    return this.answered(key, seq, run);
  }

  private wait(run: Run, limit: number): Promise<void> {
    if (run.stopped || !this.live() || this.link.backlog() <= limit) return Promise.resolve();
    return new Promise(resolve => {
      const timer = setInterval(() => {
        if (!run.stopped && this.live() && this.link.backlog() > limit) return;
        done();
      }, ROOM_EVERY_MS);
      const done = () => {
        clearInterval(timer);
        if (this.room === done) this.room = null;
        resolve();
      };
      this.room = done;
    });
  }

  private answered(key: number, seq: number, run: Run): Promise<boolean> {
    const file = this.store.get().find(entry => entry.key === key);
    if (run.stopped || !file || file.seq !== seq) return Promise.resolve(false);
    if (file.phase !== 'sent') return Promise.resolve(file.phase === 'done');
    return new Promise(resolve => {
      const timer = setTimeout(() => {
        this.fail(key, seq, UNANSWERED);
        settle(false);
      }, ANSWER_MS);
      const settle = (kept: boolean) => {
        clearTimeout(timer);
        if (this.waiting && this.waiting.seq === seq) this.waiting = null;
        resolve(kept);
      };
      this.waiting = { seq, settle };
    });
  }
}
