import { atLeast } from './chat';
import type { AttachedMsg, AttachMsg } from './protocol';

export type Phase = 'queued' | 'sending' | 'sent' | 'done' | 'failed';
export type RefusedFor = 'size' | 'count';

export type Picked = {
  uri: string;
  name: string;
  size: number;
};

export type Asset = {
  uri: string;
  name?: string | null;
  size?: number | null;
};

export type Attachment = Picked & {
  key: number;
  seq: number | null;
  phase: Phase;
  sent: number;
  path: string | null;
  problem: string | null;
};

export type Refusal = {
  name: string;
  why: RefusedFor;
};

export type Added = {
  list: Attachment[];
  refused: Refusal[];
};

export type Range = {
  offset: number;
  length: number;
};

export const PART_BYTES = 192 * 1024;
export const MAX_FILE_BYTES = 25 * 1024 * 1024;
export const MAX_ATTACHMENTS = 20;
export const NAME_MAX = 120;
export const LABEL_MAX = 22;
export const UNREADABLE = 'The phone could not read this file';
export const CHANGED = 'The file changed while it was sent';
export const DROPPED = 'The connection dropped. Send it again.';
export const UNANSWERED = 'The computer did not confirm the file. Send it again.';
export const NOT_KEPT = 'The computer did not keep the file';
export const UNKNOWN_SIZE = -1;
const UNKNOWN_LABEL = 'size unknown';
const ATTACH_SINCE = [0, 2, 39];
const FALLBACK_NAME = 'file';
const SEPARATOR = /[\\/]/;
const CONTROL = /[\u0000-\u001f\u007f]/g;
const ELLIPSIS = '…';
const KIB = 1024;
const MIB = KIB * KIB;
const ALPHABET = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';
const PAD = '=';
const ENCODE_STEP = 3 * 4096;

export function attachCapable(version: string | null): boolean {
  return atLeast(version, ATTACH_SINCE);
}

export function wireName(name: string): string {
  const base = name.split(SEPARATOR).filter(Boolean).pop() || '';
  return [...base.replace(CONTROL, '').trim()].slice(0, NAME_MAX).join('') || FALLBACK_NAME;
}

export function fileLabel(name: string, max: number = LABEL_MAX): string {
  const symbols = [...wireName(name)];
  if (symbols.length <= max) return symbols.join('');
  const dot = symbols.lastIndexOf('.');
  const tail = dot > 0 && symbols.length - dot <= max / 2 ? symbols.slice(dot - 2) : symbols.slice(-3);
  const head = Math.max(1, max - tail.length - 1);
  return `${symbols.slice(0, head).join('')}${ELLIPSIS}${tail.join('')}`;
}

export function sizeLabel(bytes: number): string {
  if (bytes < 0) return UNKNOWN_LABEL;
  if (bytes < KIB) return `${bytes} B`;
  if (bytes < MIB) return `${Math.round(bytes / KIB)} KB`;
  const mega = bytes / MIB;
  return `${mega < 10 ? mega.toFixed(1) : Math.round(mega)} MB`;
}

function measured(uri: string, measure: (uri: string) => number): number {
  try {
    const size = measure(uri);
    return typeof size === 'number' && Number.isFinite(size) && size >= 0 ? size : UNKNOWN_SIZE;
  } catch {
    return UNKNOWN_SIZE;
  }
}

export function picked(assets: Asset[], measure: (uri: string) => number): Picked[] {
  return assets.map(asset => ({ uri: asset.uri, name: wireName(asset.name || asset.uri), size: typeof asset.size === 'number' && asset.size >= 0 ? asset.size : measured(asset.uri, measure) }));
}

export function withFiles(list: Attachment[], picked: Picked[], firstKey: number): Added {
  const next = list.slice();
  const refused: Refusal[] = [];
  let key = firstKey;
  for (const file of picked) {
    if (next.some(known => known.uri === file.uri)) continue;
    if (file.size > MAX_FILE_BYTES) {
      refused.push({ name: file.name, why: 'size' });
      continue;
    }
    if (next.length >= MAX_ATTACHMENTS) {
      refused.push({ name: file.name, why: 'count' });
      continue;
    }
    next.push({ uri: file.uri, name: wireName(file.name), size: file.size < 0 ? UNKNOWN_SIZE : file.size, key, seq: null, phase: 'queued', sent: 0, path: null, problem: null });
    key += 1;
  }
  return { list: next, refused };
}

export function refusalText(refused: Refusal[]): string | null {
  const big = refused.filter(refusal => refusal.why === 'size');
  const many = refused.some(refusal => refusal.why === 'count');
  const limit = sizeLabel(MAX_FILE_BYTES);
  const size = big.length === 1 ? `${fileLabel(big[0].name)} is over ${limit} and was left out.` : big.length ? `${big.length} files are over ${limit} and were left out.` : '';
  const count = many ? `A task takes up to ${MAX_ATTACHMENTS} attached files.` : '';
  return [size, count].filter(Boolean).join(' ') || null;
}

export function partCount(size: number): number {
  return Math.max(1, Math.ceil(size / PART_BYTES));
}

export function partRange(size: number, part: number): Range {
  const offset = part * PART_BYTES;
  return { offset, length: Math.max(0, Math.min(PART_BYTES, size - offset)) };
}

export function base64(bytes: Uint8Array): string {
  const chunks: string[] = [];
  for (let start = 0; start < bytes.length; start += ENCODE_STEP) {
    const end = Math.min(bytes.length, start + ENCODE_STEP);
    let chunk = '';
    for (let at = start; at < end; at += 3) {
      const left = end - at;
      const triple = (bytes[at] << 16) | ((left > 1 ? bytes[at + 1] : 0) << 8) | (left > 2 ? bytes[at + 2] : 0);
      chunk += ALPHABET[(triple >> 18) & 63] + ALPHABET[(triple >> 12) & 63] + (left > 1 ? ALPHABET[(triple >> 6) & 63] : PAD) + (left > 2 ? ALPHABET[triple & 63] : PAD);
    }
    chunks.push(chunk);
  }
  return chunks.join('');
}

export function attachMessage(seq: number, name: string, part: number, last: boolean, data: string): AttachMsg {
  return { type: 'attach', seq, name, part, last, data };
}

function change(list: Attachment[], key: number, part: Partial<Attachment>): Attachment[] {
  return list.some(file => file.key === key) ? list.map(file => (file.key === key ? { ...file, ...part } : file)) : list;
}

export function started(list: Attachment[], key: number, seq: number): Attachment[] {
  return change(list, key, { seq, phase: 'sending', sent: 0, path: null, problem: null });
}

export function progressed(list: Attachment[], key: number, sent: number, last: boolean): Attachment[] {
  const file = list.find(entry => entry.key === key);
  if (!file || file.phase !== 'sending') return list;
  return change(list, key, { sent: Math.min(file.size, sent), phase: last ? 'sent' : 'sending' });
}

export function failed(list: Attachment[], key: number, problem: string): Attachment[] {
  return change(list, key, { phase: 'failed', problem, path: null });
}

export function heard(list: Attachment[], message: AttachedMsg): Attachment[] {
  const file = list.find(entry => entry.seq === message.seq && (entry.phase === 'sending' || entry.phase === 'sent'));
  if (!file) return list;
  if (message.problem || !message.path) return failed(list, file.key, message.problem || NOT_KEPT);
  if (file.phase !== 'sent') return list;
  return change(list, file.key, { phase: 'done', sent: file.size, path: message.path });
}

export function cancelled(list: Attachment[]): Attachment[] {
  if (!uploading(list)) return list;
  return list.map(file => (file.phase === 'sending' || file.phase === 'sent' ? { ...file, phase: 'queued', seq: null, sent: 0 } : file));
}

export function interrupted(list: Attachment[], problem: string): Attachment[] {
  if (!uploading(list)) return list;
  return list.map(file => (file.phase === 'sending' || file.phase === 'sent' ? { ...file, phase: 'failed', problem, path: null } : file));
}

export function removed(list: Attachment[], key: number): Attachment[] {
  return list.some(file => file.key === key) ? list.filter(file => file.key !== key) : list;
}

export function uploading(list: Attachment[]): boolean {
  return list.some(file => file.phase === 'sending' || file.phase === 'sent');
}

export function pending(list: Attachment[]): Attachment[] {
  return list.filter(file => file.phase !== 'done');
}

export function paths(list: Attachment[]): string[] | null {
  const found: string[] = [];
  for (const file of list) {
    if (file.phase !== 'done' || !file.path) return null;
    found.push(file.path);
  }
  return found;
}

export function progress(file: Attachment): number {
  if (file.phase === 'done' || file.phase === 'sent') return 1;
  if (file.phase !== 'sending' || !file.size) return 0;
  return Math.min(1, file.sent / file.size);
}

export function chipNote(file: Attachment): string {
  if (file.phase === 'failed') return file.problem || NOT_KEPT;
  if (file.phase === 'sending') return `${Math.floor(progress(file) * 100)}%`;
  if (file.phase === 'sent') return 'saving…';
  return file.phase === 'done' ? `${sizeLabel(file.size)} · sent` : sizeLabel(file.size);
}

export function sendLabel(list: Attachment[]): string | null {
  if (!uploading(list)) return null;
  const done = list.filter(file => file.phase === 'done').length;
  return `Sending ${Math.min(list.length, done + 1)} of ${list.length}`;
}
