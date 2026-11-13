import type { Attachment, Picked } from '../src/core/attach';
import {
  attachCapable,
  attachMessage,
  base64,
  cancelled,
  CHANGED,
  chipNote,
  DROPPED,
  failed,
  fileLabel,
  heard,
  interrupted,
  MAX_ATTACHMENTS,
  MAX_FILE_BYTES,
  NOT_KEPT,
  PART_BYTES,
  partCount,
  partRange,
  paths,
  pending,
  picked,
  progress,
  progressed,
  refusalText,
  removed,
  sendLabel,
  sizeLabel,
  started,
  UNANSWERED,
  UNKNOWN_SIZE,
  UNREADABLE,
  uploading,
  wireName,
  withFiles,
} from '../src/core/attach';
import type { Identity, Transport } from '../src/core/link';
import { DeviceLink } from '../src/core/link';
import type { AttachMsg, BridgeMsg, PhoneMsg } from '../src/core/protocol';
import type { Opener } from '../src/core/uploads';
import { ANSWER_MS, HIGH_WATER, ROOM_EVERY_MS, Uploads } from '../src/core/uploads';

const IDENTITY: Identity = { agent: 'Octo/1.0.0', device: { type: 'device', model: 'Pixel 9', osVersion: '16', standalone: true, client: 'phone-1' } };
const MIB = 1024 * 1024;
const FIRST_SEQ = 100;

function file(part: Partial<Attachment>): Attachment {
  return { uri: 'file:///a.png', name: 'a.png', size: 1000, key: 1, seq: null, phase: 'queued', sent: 0, path: null, problem: null, ...part };
}

function chosen(name: string, size: number): Picked {
  return { uri: `file:///cache/${name}`, name, size };
}

function bytesOf(size: number): Uint8Array {
  return Uint8Array.from({ length: size }, (_, index) => index % 251);
}

function decoded(data: string): number[] {
  return Array.from(atob(data), symbol => symbol.charCodeAt(0));
}

function reference(bytes: Uint8Array): string {
  return btoa(Array.from(bytes, byte => String.fromCharCode(byte)).join(''));
}

type Setup = {
  uploads: Uploads;
  link: DeviceLink;
  sent: PhoneMsg[];
  parts: () => AttachMsg[];
  hear: (message: BridgeMsg) => void;
  drop: () => void;
  reads: number[][];
  closed: string[];
  disk: Map<string, Uint8Array>;
  backlog: { bytes: number; fills: boolean };
};

function setup(): Setup {
  const sent: PhoneMsg[] = [];
  const backlog = { bytes: 0, fills: false };
  let session = '';
  const wire: Transport = {
    open: id => {
      session = id;
    },
    send: (_id, text) => {
      sent.push(JSON.parse(text));
      if (backlog.fills) backlog.bytes = HIGH_WATER + 1;
    },
    queued: () => backlog.bytes,
    close: () => {},
  };
  const link = new DeviceLink('home', 'https://h', 'A'.repeat(43), wire, IDENTITY);
  link.connect();
  link.onReady(session);
  const hear = (message: BridgeMsg) => link.onMessage(session, JSON.stringify(message));
  hear({ type: 'welcome', host: 'lucas-desktop', version: '0.2.39' });
  sent.length = 0;
  const disk = new Map<string, Uint8Array>();
  const reads: number[][] = [];
  const closed: string[] = [];
  const open: Opener = uri => {
    const stored = disk.get(uri);
    if (!stored) throw new Error('gone');
    return {
      read: async (offset, length) => {
        reads.push([offset, length]);
        return stored.slice(offset, offset + length);
      },
      close: () => closed.push(uri),
    };
  };
  const uploads = new Uploads(link, open, FIRST_SEQ);
  link.listen(message => {
    if (message.type === 'attached') uploads.hear(message);
  });
  const parts = () => sent.filter((message): message is AttachMsg => message.type === 'attach');
  return { uploads, link, sent, parts, hear, drop: () => link.onClosed(session, 1006), reads, closed, disk, backlog };
}

async function settle(): Promise<void> {
  for (let turn = 0; turn < 20; turn += 1) await Promise.resolve();
}

describe('the files of a task', () => {
  it('are only offered to an Octo that knows how to receive them', () => {
    expect(['0.2.39', '0.2.40', '0.3.0', '1.0.0'].map(attachCapable)).toEqual([true, true, true, true]);
    expect(['0.2.38', '0.2.36', '0.1.99', '', 'dev'].map(attachCapable)).toEqual([false, false, false, false, false]);
    expect(attachCapable(null)).toBe(false);
  });

  it('travel under their base name, without separators or control characters, at most 120 characters long', () => {
    expect(wireName('C:\\Users\\lucas\\shot.png')).toBe('shot.png');
    expect(wireName('/storage/emulated/0/DCIM/IMG 0042.jpg')).toBe('IMG 0042.jpg');
    expect(wireName('re\u0000po\u001frt\u007f.pdf')).toBe('report.pdf');
    expect(wireName('')).toBe('file');
    expect(wireName('///')).toBe('file');
    expect(wireName('  \u0007 ')).toBe('file');
    expect([...wireName(`${'é'.repeat(200)}.txt`)]).toHaveLength(120);
  });

  it('show a short name that keeps the start and the extension', () => {
    expect(fileLabel('notes.md')).toBe('notes.md');
    expect(fileLabel('screenshot-2026-11-13-at-10.42.07.png')).toBe('screenshot-2026…07.png');
    expect([...fileLabel('screenshot-2026-11-13-at-10.42.07.png')]).toHaveLength(22);
    expect(fileLabel('a-very-long-name-without-any-extension')).toBe('a-very-long-name-w…ion');
    expect(fileLabel('archive.with.a.really.long.extension-of-its-own', 12)).toBe('archive.…own');
    expect(fileLabel('/tmp/x/photo.jpg')).toBe('photo.jpg');
  });

  it('show their size the way a person reads it', () => {
    expect([0, 812, 1024, 1536, 300 * 1024, MIB, 1.44 * MIB, 12.6 * MIB, MAX_FILE_BYTES].map(sizeLabel)).toEqual(['0 B', '812 B', '1 KB', '2 KB', '300 KB', '1.0 MB', '1.4 MB', '13 MB', '25 MB']);
  });

  it('come from the pickers with a name and a size, measured when the picker did not say', () => {
    const measure = jest.fn(() => 4096);
    expect(
      picked(
        [
          { uri: 'file:///cache/IMG_1.jpg', name: 'IMG_1.jpg', size: 2048 },
          { uri: 'file:///cache/ImagePicker/9f2c.mp4', name: null, size: undefined },
          { uri: 'file:///cache/empty.txt', name: 'empty.txt', size: 0 },
        ],
        measure,
      ),
    ).toEqual([
      { uri: 'file:///cache/IMG_1.jpg', name: 'IMG_1.jpg', size: 2048 },
      { uri: 'file:///cache/ImagePicker/9f2c.mp4', name: '9f2c.mp4', size: 4096 },
      { uri: 'file:///cache/empty.txt', name: 'empty.txt', size: 0 },
    ]);
    expect(measure).toHaveBeenCalledTimes(1);
  });

  it('are marked as of unknown size when the phone cannot measure them, never as empty', () => {
    const broken = () => {
      throw new Error('gone');
    };
    expect(picked([{ uri: 'file:///cache/a.bin', name: 'a.bin' }], broken)).toEqual([{ uri: 'file:///cache/a.bin', name: 'a.bin', size: UNKNOWN_SIZE }]);
    expect(picked([{ uri: 'file:///cache/a.bin', name: 'a.bin', size: null }], () => Number.NaN)[0].size).toBe(UNKNOWN_SIZE);
    expect(picked([{ uri: 'file:///cache/a.bin', name: 'a.bin', size: -4 }], () => 7)[0].size).toBe(7);
    const kept = withFiles([], [{ uri: 'file:///cache/a.bin', name: 'a.bin', size: UNKNOWN_SIZE }], 1).list[0];
    expect(kept.size).toBe(UNKNOWN_SIZE);
    expect(sizeLabel(kept.size)).toBe('size unknown');
  });

  it('join the task once each, with a key that never repeats', () => {
    const first = withFiles([], [chosen('a.png', 10), chosen('b.pdf', 20), chosen('a.png', 10)], 1);
    expect(first.list.map(entry => [entry.key, entry.name, entry.phase])).toEqual([
      [1, 'a.png', 'queued'],
      [2, 'b.pdf', 'queued'],
    ]);
    expect(first.refused).toEqual([]);
    const second = withFiles(first.list, [chosen('b.pdf', 20), chosen('c.txt', 0)], 3);
    expect(second.list.map(entry => entry.key)).toEqual([1, 2, 3]);
  });

  it('leave out a file over 25 MiB and everything past the twentieth, saying so plainly', () => {
    const big = withFiles([], [chosen('movie.mov', MAX_FILE_BYTES + 1), chosen('edge.bin', MAX_FILE_BYTES)], 1);
    expect(big.list.map(entry => entry.name)).toEqual(['edge.bin']);
    expect(big.refused).toEqual([{ name: 'movie.mov', why: 'size' }]);
    expect(refusalText(big.refused)).toBe('movie.mov is over 25 MB and was left out.');
    const many = withFiles([], Array.from({ length: MAX_ATTACHMENTS + 3 }, (_, index) => chosen(`f${index}.txt`, 1)), 1);
    expect(many.list).toHaveLength(MAX_ATTACHMENTS);
    expect(many.refused).toHaveLength(3);
    expect(refusalText(many.refused)).toBe('A task takes up to 20 attached files.');
    const both = withFiles(many.list, [chosen('x.mov', MAX_FILE_BYTES * 2), chosen('y.mov', MAX_FILE_BYTES * 2), chosen('z.txt', 1)], 90);
    expect(refusalText(both.refused)).toBe('2 files are over 25 MB and were left out. A task takes up to 20 attached files.');
    expect(refusalText([])).toBeNull();
  });

  it('are cut in parts of 192 KiB, the last one shorter, and an empty file is still one part', () => {
    expect([PART_BYTES, MAX_FILE_BYTES, MAX_ATTACHMENTS]).toEqual([196_608, 26_214_400, 20]);
    expect([0, 1, PART_BYTES, PART_BYTES + 1, MAX_FILE_BYTES].map(partCount)).toEqual([1, 1, 1, 2, 134]);
    expect(partRange(0, 0)).toEqual({ offset: 0, length: 0 });
    expect(partRange(PART_BYTES + 5, 0)).toEqual({ offset: 0, length: PART_BYTES });
    expect(partRange(PART_BYTES + 5, 1)).toEqual({ offset: PART_BYTES, length: 5 });
    expect(partRange(PART_BYTES, 1)).toEqual({ offset: PART_BYTES, length: 0 });
  });

  it('are written in base64 exactly as the computer reads it', () => {
    for (const size of [0, 1, 2, 3, 4, 5, 255, 256, 12288, 12289, PART_BYTES]) {
      const bytes = bytesOf(size);
      expect(base64(bytes)).toBe(reference(bytes));
    }
    expect(base64(Uint8Array.from([255, 254, 253]))).toBe('//79');
    expect(attachMessage(7, 'a.png', 2, true, 'QQ==')).toEqual({ type: 'attach', seq: 7, name: 'a.png', part: 2, last: true, data: 'QQ==' });
  });
});

describe('the upload of a file', () => {
  it('goes from queued to sending, to sent once the last part left, to done when the computer says where it is', () => {
    let list = [file({ size: 1000 })];
    list = started(list, 1, 41);
    expect(list[0]).toMatchObject({ phase: 'sending', seq: 41, sent: 0 });
    list = progressed(list, 1, 400, false);
    expect(list[0]).toMatchObject({ phase: 'sending', sent: 400 });
    expect(progress(list[0])).toBe(0.4);
    expect(chipNote(list[0])).toBe('40%');
    list = progressed(list, 1, 1000, true);
    expect(list[0]).toMatchObject({ phase: 'sent', sent: 1000 });
    expect(chipNote(list[0])).toBe('saving…');
    expect(uploading(list)).toBe(true);
    list = heard(list, { type: 'attached', seq: 41, path: 'C:\\octo\\attachments\\x\\a.png', problem: null });
    expect(list[0]).toMatchObject({ phase: 'done', path: 'C:\\octo\\attachments\\x\\a.png' });
    expect(chipNote(list[0])).toBe('1000 B · sent');
    expect(progress(list[0])).toBe(1);
    expect(uploading(list)).toBe(false);
    expect(paths(list)).toEqual(['C:\\octo\\attachments\\x\\a.png']);
  });

  it('fails with the words of the computer, even while parts are still leaving', () => {
    const sending = progressed(started([file({})], 1, 41), 1, 300, false);
    const refused = heard(sending, { type: 'attached', seq: 41, path: null, problem: 'the file is over 25 MiB' });
    expect(refused[0]).toMatchObject({ phase: 'failed', problem: 'the file is over 25 MiB', path: null });
    expect(chipNote(refused[0])).toBe('the file is over 25 MiB');
    expect(progress(refused[0])).toBe(0);
    expect(paths(refused)).toBeNull();
    const silent = heard(sending, { type: 'attached', seq: 41, path: null, problem: null });
    expect(silent[0]).toMatchObject({ phase: 'failed', problem: NOT_KEPT });
  });

  it('ignores an answer about another upload, an older attempt, or a place given before the last part left', () => {
    const sending = progressed(started([file({}), file({ key: 2, uri: 'file:///b' })], 1, 41), 1, 300, false);
    expect(heard(sending, { type: 'attached', seq: 40, path: '/x', problem: null })).toBe(sending);
    expect(heard(sending, { type: 'attached', seq: 99, path: null, problem: 'nope' })).toBe(sending);
    expect(heard(sending, { type: 'attached', seq: 41, path: '/early', problem: null })).toBe(sending);
    const done = heard(progressed(sending, 1, 1000, true), { type: 'attached', seq: 41, path: '/p', problem: null });
    expect(heard(done, { type: 'attached', seq: 41, path: null, problem: 'late' })).toBe(done);
    expect(progressed(done, 1, 10, false)).toBe(done);
    expect(progressed(done, 9, 10, false)).toBe(done);
  });

  it('goes back to the queue when cancelled, keeps what was already sent, and fails when the connection drops', () => {
    const first = heard(progressed(started([file({}), file({ key: 2, uri: 'file:///b', name: 'b.pdf' })], 1, 41), 1, 1000, true), { type: 'attached', seq: 41, path: '/p/a.png', problem: null });
    const second = progressed(started(first, 2, 42), 2, 500, false);
    const stopped = cancelled(second);
    expect(stopped.map(entry => [entry.phase, entry.sent, entry.seq])).toEqual([
      ['done', 1000, 41],
      ['queued', 0, null],
    ]);
    expect(cancelled(stopped)).toBe(stopped);
    const lost = interrupted(second, DROPPED);
    expect(lost.map(entry => [entry.phase, entry.problem])).toEqual([
      ['done', null],
      ['failed', DROPPED],
    ]);
    expect(interrupted(lost, DROPPED)).toBe(lost);
    expect(pending(lost).map(entry => entry.key)).toEqual([2]);
    expect(started(lost, 2, 43)[1]).toMatchObject({ phase: 'sending', seq: 43, sent: 0, problem: null });
  });

  it('leaves the list alone for a file that is not there', () => {
    const list = [file({})];
    expect(removed(list, 9)).toBe(list);
    expect(started(list, 9, 1)).toBe(list);
    expect(failed(list, 9, 'x')).toBe(list);
    expect(removed(list, 1)).toEqual([]);
  });

  it('tells the composer which file is leaving, and nothing when none is', () => {
    const list = [file({ phase: 'done', path: '/a' }), file({ key: 2, phase: 'sending', seq: 2 }), file({ key: 3 })];
    expect(sendLabel(list)).toBe('Sending 2 of 3');
    expect(sendLabel([file({})])).toBeNull();
    expect(sendLabel([])).toBeNull();
    expect(paths([])).toEqual([]);
    expect(chipNote(file({ size: 3 * MIB }))).toBe('3.0 MB');
    expect(progress(file({ phase: 'sending', size: 0 }))).toBe(0);
  });
});

describe('sending the files to the computer', () => {
  beforeEach(() => jest.useFakeTimers());
  afterEach(() => jest.useRealTimers());

  it('reads a file part by part, sends the parts in order and gives back the place the computer kept it', async () => {
    const { uploads, parts, hear, reads, closed, disk } = setup();
    const size = PART_BYTES * 2 + 100;
    disk.set('file:///cache/video.mp4', bytesOf(size));
    expect(uploads.add([chosen('video.mp4', size)])).toEqual([]);
    const done = uploads.send();
    await settle();
    expect(reads).toEqual([
      [0, PART_BYTES],
      [PART_BYTES, PART_BYTES],
      [PART_BYTES * 2, 100],
      [size, 1],
    ]);
    expect(parts().map(part => [part.seq, part.name, part.part, part.last])).toEqual([
      [FIRST_SEQ + 1, 'video.mp4', 0, false],
      [FIRST_SEQ + 1, 'video.mp4', 1, false],
      [FIRST_SEQ + 1, 'video.mp4', 2, true],
    ]);
    expect(parts().map(part => part.data).join('')).toBe(reference(bytesOf(size)));
    expect(parts().map(part => decoded(part.data).length)).toEqual([PART_BYTES, PART_BYTES, 100]);
    expect(uploads.store.get()[0]).toMatchObject({ phase: 'sent', sent: size });
    expect(closed).toEqual(['file:///cache/video.mp4']);
    hear({ type: 'attached', seq: FIRST_SEQ + 1, path: '/home/attachments/k/video.mp4', problem: null });
    await expect(done).resolves.toEqual(['/home/attachments/k/video.mp4']);
  });

  it('sends one file after the other, each waiting for its answer, and an empty file as one empty last part', async () => {
    const { uploads, parts, hear, disk } = setup();
    disk.set('file:///cache/a.txt', bytesOf(5));
    disk.set('file:///cache/empty.log', bytesOf(0));
    uploads.add([chosen('a.txt', 5), chosen('empty.log', 0)]);
    const done = uploads.send();
    await settle();
    expect(parts()).toHaveLength(1);
    hear({ type: 'attached', seq: FIRST_SEQ + 1, path: '/k/a.txt', problem: null });
    await settle();
    expect(parts()[1]).toEqual({ type: 'attach', seq: FIRST_SEQ + 2, name: 'empty.log', part: 0, last: true, data: '' });
    hear({ type: 'attached', seq: FIRST_SEQ + 2, path: '/k/empty.log', problem: null });
    await expect(done).resolves.toEqual(['/k/a.txt', '/k/empty.log']);
  });

  it('stops at the file the computer refuses and shows why, then sends only what is missing on the next try', async () => {
    const { uploads, parts, hear, disk } = setup();
    disk.set('file:///cache/a.txt', bytesOf(5));
    disk.set('file:///cache/b.txt', bytesOf(6));
    disk.set('file:///cache/c.txt', bytesOf(7));
    uploads.add([chosen('a.txt', 5), chosen('b.txt', 6), chosen('c.txt', 7)]);
    const first = uploads.send();
    await settle();
    hear({ type: 'attached', seq: FIRST_SEQ + 1, path: '/k/a.txt', problem: null });
    await settle();
    hear({ type: 'attached', seq: FIRST_SEQ + 2, path: null, problem: 'the disk of the computer is full' });
    await expect(first).resolves.toBeNull();
    expect(uploads.store.get().map(entry => [entry.phase, entry.problem])).toEqual([
      ['done', null],
      ['failed', 'the disk of the computer is full'],
      ['queued', null],
    ]);
    expect(parts()).toHaveLength(2);
    const again = uploads.send();
    await settle();
    expect(parts()[2]).toMatchObject({ seq: FIRST_SEQ + 3, name: 'b.txt' });
    hear({ type: 'attached', seq: FIRST_SEQ + 2, path: '/late', problem: null });
    hear({ type: 'attached', seq: FIRST_SEQ + 3, path: '/k/b.txt', problem: null });
    await settle();
    hear({ type: 'attached', seq: FIRST_SEQ + 4, path: '/k/c.txt', problem: null });
    await expect(again).resolves.toEqual(['/k/a.txt', '/k/b.txt', '/k/c.txt']);
  });

  it('stops sending parts as soon as the computer says the upload failed', async () => {
    const { uploads, parts, hear, disk, backlog } = setup();
    const size = PART_BYTES * 3;
    disk.set('file:///cache/big.bin', bytesOf(size));
    uploads.add([chosen('big.bin', size)]);
    backlog.fills = true;
    const done = uploads.send();
    await settle();
    expect(parts().map(part => part.part)).toEqual([0]);
    hear({ type: 'attached', seq: FIRST_SEQ + 1, path: null, problem: 'the file is over 25 MiB' });
    backlog.fills = false;
    backlog.bytes = 0;
    jest.advanceTimersByTime(ROOM_EVERY_MS);
    await expect(done).resolves.toBeNull();
    expect(parts().map(part => part.part)).toEqual([0]);
    expect(uploads.store.get()[0]).toMatchObject({ phase: 'failed', problem: 'the file is over 25 MiB' });
  });

  it('waits while the connection still holds more than half a megabyte, so a big file never piles up in memory', async () => {
    const { uploads, parts, hear, disk, backlog } = setup();
    const size = PART_BYTES * 2;
    disk.set('file:///cache/big.bin', bytesOf(size));
    uploads.add([chosen('big.bin', size)]);
    backlog.bytes = HIGH_WATER + 1;
    const done = uploads.send();
    await settle();
    jest.advanceTimersByTime(ROOM_EVERY_MS * 10);
    await settle();
    expect(parts()).toHaveLength(0);
    backlog.bytes = HIGH_WATER;
    jest.advanceTimersByTime(ROOM_EVERY_MS);
    await settle();
    expect(parts()).toHaveLength(2);
    hear({ type: 'attached', seq: FIRST_SEQ + 1, path: '/k/big.bin', problem: null });
    await expect(done).resolves.toEqual(['/k/big.bin']);
  });

  it('can be cancelled half way: nothing else leaves, the file waits in the queue and the late answer is ignored', async () => {
    const { uploads, parts, hear, disk, backlog } = setup();
    const size = PART_BYTES * 3;
    disk.set('file:///cache/big.bin', bytesOf(size));
    uploads.add([chosen('big.bin', size)]);
    backlog.bytes = HIGH_WATER + 1;
    const done = uploads.send();
    await settle();
    uploads.cancel();
    backlog.bytes = 0;
    await expect(done).resolves.toBeNull();
    expect(parts()).toHaveLength(0);
    expect(uploads.store.get()[0]).toMatchObject({ phase: 'queued', sent: 0, seq: null });
    hear({ type: 'attached', seq: FIRST_SEQ + 1, path: '/k/big.bin', problem: null });
    expect(uploads.store.get()[0].phase).toBe('queued');
    jest.advanceTimersByTime(ANSWER_MS * 2);
    expect(uploads.store.get()[0].phase).toBe('queued');
  });

  it('is cancelled by removing the file that is leaving, while removing another one lets it go on', async () => {
    const { uploads, hear, disk } = setup();
    disk.set('file:///cache/a.txt', bytesOf(5));
    disk.set('file:///cache/b.txt', bytesOf(6));
    uploads.add([chosen('a.txt', 5), chosen('b.txt', 6)]);
    const first = uploads.send();
    await settle();
    uploads.remove(2);
    hear({ type: 'attached', seq: FIRST_SEQ + 1, path: '/k/a.txt', problem: null });
    await expect(first).resolves.toEqual(['/k/a.txt']);
    uploads.add([chosen('b.txt', 6)]);
    const second = uploads.send();
    await settle();
    uploads.remove(uploads.store.get()[1].key);
    await expect(second).resolves.toBeNull();
    expect(uploads.store.get().map(entry => entry.name)).toEqual(['a.txt']);
  });

  it('gives up when the computer never confirms, when the connection drops and when the phone cannot read the file', async () => {
    const quiet = setup();
    quiet.disk.set('file:///cache/a.txt', bytesOf(5));
    quiet.uploads.add([chosen('a.txt', 5)]);
    const unanswered = quiet.uploads.send();
    await settle();
    jest.advanceTimersByTime(ANSWER_MS);
    await expect(unanswered).resolves.toBeNull();
    expect(quiet.uploads.store.get()[0]).toMatchObject({ phase: 'failed', problem: UNANSWERED });

    const dropped = setup();
    dropped.disk.set('file:///cache/a.txt', bytesOf(5));
    dropped.uploads.add([chosen('a.txt', 5)]);
    const lost = dropped.uploads.send();
    await settle();
    dropped.uploads.reset();
    await expect(lost).resolves.toBeNull();
    expect(dropped.uploads.store.get()[0]).toMatchObject({ phase: 'failed', problem: DROPPED });

    const offline = setup();
    offline.disk.set('file:///cache/a.txt', bytesOf(5));
    offline.uploads.add([chosen('a.txt', 5)]);
    offline.drop();
    await expect(offline.uploads.send()).resolves.toBeNull();
    expect(offline.parts()).toEqual([]);
    expect(offline.uploads.store.get()[0]).toMatchObject({ phase: 'failed', problem: DROPPED });

    const missing = setup();
    missing.uploads.add([chosen('gone.txt', 5)]);
    await expect(missing.uploads.send()).resolves.toBeNull();
    expect(missing.uploads.store.get()[0]).toMatchObject({ phase: 'failed', problem: UNREADABLE });

    const shrunk = setup();
    shrunk.disk.set('file:///cache/a.txt', bytesOf(3));
    shrunk.uploads.add([chosen('a.txt', 5)]);
    await expect(shrunk.uploads.send()).resolves.toBeNull();
    expect(shrunk.parts()).toEqual([]);
    expect(shrunk.uploads.store.get()[0]).toMatchObject({ phase: 'failed', problem: CHANGED });
    expect(shrunk.closed).toEqual(['file:///cache/a.txt']);
  });

  it('never runs twice at once, and has nothing to send for an empty list', async () => {
    const { uploads, hear, disk } = setup();
    await expect(uploads.send()).resolves.toEqual([]);
    disk.set('file:///cache/a.txt', bytesOf(5));
    uploads.add([chosen('a.txt', 5)]);
    const first = uploads.send();
    await expect(uploads.send()).resolves.toBeNull();
    await settle();
    hear({ type: 'attached', seq: FIRST_SEQ + 1, path: '/k/a.txt', problem: null });
    await expect(first).resolves.toEqual(['/k/a.txt']);
    uploads.clear();
    expect(uploads.store.get()).toEqual([]);
  });

  it('keeps the limits when files are added in several rounds', () => {
    const { uploads } = setup();
    expect(uploads.add(Array.from({ length: 19 }, (_, index) => chosen(`f${index}.txt`, 1)))).toEqual([]);
    expect(uploads.add([chosen('big.mov', MAX_FILE_BYTES + 1), chosen('ok.txt', 1), chosen('over.txt', 1)])).toEqual([
      { name: 'big.mov', why: 'size' },
      { name: 'over.txt', why: 'count' },
    ]);
    expect(uploads.store.get()).toHaveLength(MAX_ATTACHMENTS);
    expect(new Set(uploads.store.get().map(entry => entry.key)).size).toBe(MAX_ATTACHMENTS);
  });

  it('tells the computer to drop an upload it abandons half way, so the next ones still fit', async () => {
    const size = PART_BYTES * 3;
    const half = async () => {
      const made = setup();
      made.disk.set('file:///cache/big.bin', bytesOf(size));
      made.uploads.add([chosen('big.bin', size)]);
      made.backlog.fills = true;
      const done = made.uploads.send();
      await settle();
      expect(made.parts().map(part => part.part)).toEqual([0]);
      return { ...made, done };
    };
    const release = { type: 'attach', seq: FIRST_SEQ + 1, name: 'big.bin', part: 0, last: true, data: '' };

    const stopped = await half();
    stopped.uploads.cancel();
    await expect(stopped.done).resolves.toBeNull();
    expect(stopped.parts()).toHaveLength(2);
    expect(stopped.parts()[1]).toEqual(release);
    stopped.uploads.cancel();
    expect(stopped.parts()).toHaveLength(2);

    const taken = await half();
    taken.uploads.remove(1);
    await expect(taken.done).resolves.toBeNull();
    expect(taken.parts()[1]).toEqual(release);

    const closed = await half();
    closed.uploads.dispose();
    await expect(closed.done).resolves.toBeNull();
    expect(closed.parts()[1]).toEqual(release);

    const refused = await half();
    refused.hear({ type: 'attached', seq: FIRST_SEQ + 1, path: null, problem: 'the disk is full' });
    refused.uploads.cancel();
    await expect(refused.done).resolves.toBeNull();
    expect(refused.parts()).toHaveLength(1);

    const away = await half();
    away.drop();
    away.uploads.cancel();
    await expect(away.done).resolves.toBeNull();
    expect(away.parts()).toHaveLength(1);

    const again = await half();
    again.hear({ type: 'welcome', host: 'lucas-desktop', version: '0.2.39' });
    again.uploads.reset();
    await expect(again.done).resolves.toBeNull();
    expect(again.parts()).toHaveLength(1);
  });

  it('drops the upload on the computer when the file fails half way, and sends nothing when no part had left', async () => {
    const size = PART_BYTES * 2;
    const cut = setup();
    cut.disk.set('file:///cache/big.bin', bytesOf(PART_BYTES + 10));
    cut.uploads.add([chosen('big.bin', size)]);
    await expect(cut.uploads.send()).resolves.toBeNull();
    expect(cut.uploads.store.get()[0]).toMatchObject({ phase: 'failed', problem: CHANGED });
    expect(cut.parts().map(part => [part.part, part.last, part.data.length > 0])).toEqual([
      [0, false, true],
      [0, true, false],
    ]);

    const broken = setup();
    let reads = 0;
    const flaky = new Uploads(broken.link, () => ({
      read: async (_offset, length) => {
        reads += 1;
        if (reads > 1) throw new Error('unplugged');
        return bytesOf(length);
      },
      close: () => {},
    }), FIRST_SEQ);
    flaky.add([chosen('big.bin', size)]);
    await expect(flaky.send()).resolves.toBeNull();
    expect(flaky.store.get()[0]).toMatchObject({ phase: 'failed', problem: UNREADABLE });
    expect(broken.parts().map(part => [part.part, part.last])).toEqual([
      [0, false],
      [0, true],
    ]);

    const early = setup();
    early.disk.set('file:///cache/big.bin', bytesOf(size));
    early.uploads.add([chosen('big.bin', size)]);
    early.backlog.bytes = HIGH_WATER + 1;
    const waiting = early.uploads.send();
    await settle();
    early.uploads.cancel();
    await expect(waiting).resolves.toBeNull();
    expect(early.parts()).toEqual([]);

    const whole = setup();
    whole.disk.set('file:///cache/a.txt', bytesOf(5));
    whole.uploads.add([chosen('a.txt', 5)]);
    const sent = whole.uploads.send();
    await settle();
    whole.uploads.cancel();
    await expect(sent).resolves.toBeNull();
    expect(whole.parts()).toHaveLength(1);
  });

  it('refuses a file that is longer than it said, or whose size the phone could not read, instead of sending the wrong bytes', async () => {
    const grown = setup();
    grown.disk.set('file:///cache/a.txt', bytesOf(9));
    grown.uploads.add([chosen('a.txt', 5)]);
    await expect(grown.uploads.send()).resolves.toBeNull();
    expect(grown.parts()).toEqual([]);
    expect(grown.uploads.store.get()[0]).toMatchObject({ phase: 'failed', problem: CHANGED });

    const hollow = setup();
    hollow.disk.set('file:///cache/a.txt', bytesOf(9));
    hollow.uploads.add([chosen('a.txt', 0)]);
    await expect(hollow.uploads.send()).resolves.toBeNull();
    expect(hollow.parts()).toEqual([]);
    expect(hollow.uploads.store.get()[0]).toMatchObject({ phase: 'failed', problem: CHANGED });

    const unknown = setup();
    unknown.disk.set('file:///cache/a.txt', bytesOf(9));
    unknown.uploads.add([chosen('a.txt', UNKNOWN_SIZE)]);
    await expect(unknown.uploads.send()).resolves.toBeNull();
    expect(unknown.parts()).toEqual([]);
    expect(unknown.reads).toEqual([]);
    expect(unknown.uploads.store.get()[0]).toMatchObject({ phase: 'failed', problem: UNREADABLE });

    const locked = setup();
    const sealed = new Uploads(locked.link, () => ({ read: async () => Promise.reject(new Error('denied')), close: () => {} }), FIRST_SEQ);
    sealed.add([chosen('empty.log', 0)]);
    await expect(sealed.send()).resolves.toBeNull();
    expect(locked.parts()).toEqual([]);
    expect(sealed.store.get()[0]).toMatchObject({ phase: 'failed', problem: UNREADABLE });

    const empty = setup();
    empty.disk.set('file:///cache/empty.log', bytesOf(0));
    empty.uploads.add([chosen('empty.log', 0)]);
    const done = empty.uploads.send();
    await settle();
    expect(empty.reads).toEqual([[0, 1]]);
    expect(empty.parts()).toEqual([{ type: 'attach', seq: FIRST_SEQ + 1, name: 'empty.log', part: 0, last: true, data: '' }]);
    empty.hear({ type: 'attached', seq: FIRST_SEQ + 1, path: '/k/empty.log', problem: null });
    await expect(done).resolves.toEqual(['/k/empty.log']);
  });

  it('counts the wait for the answer from the moment the last part really left the phone', async () => {
    const { uploads, parts, hear, disk, backlog } = setup();
    disk.set('file:///cache/a.txt', bytesOf(5));
    disk.set('file:///cache/b.txt', bytesOf(6));
    uploads.add([chosen('a.txt', 5), chosen('b.txt', 6)]);
    backlog.fills = true;
    const done = uploads.send();
    await settle();
    expect(parts()).toHaveLength(1);
    jest.advanceTimersByTime(ANSWER_MS * 2);
    await settle();
    expect(uploads.store.get()[0]).toMatchObject({ phase: 'sent', problem: null });
    hear({ type: 'attached', seq: FIRST_SEQ + 1, path: '/k/a.txt', problem: null });
    await settle();
    expect(uploads.store.get()[0].phase).toBe('done');
    backlog.fills = false;
    backlog.bytes = 0;
    jest.advanceTimersByTime(ROOM_EVERY_MS);
    await settle();
    expect(parts()).toHaveLength(2);
    jest.advanceTimersByTime(ANSWER_MS - 1);
    await settle();
    expect(uploads.store.get()[1].phase).toBe('sent');
    jest.advanceTimersByTime(1);
    await expect(done).resolves.toBeNull();
    expect(uploads.store.get()[1]).toMatchObject({ phase: 'failed', problem: UNANSWERED });
  });
});
