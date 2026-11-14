import type { LinkPhase } from './link';
import { secureOrigin } from './pairing';
import { Store } from './store';

export type PushState = {
  known: boolean;
  token: string | null;
};

export type PushTarget = {
  device: string;
  agent: number;
};

export const push = new Store<PushState>({ known: false, token: null });

export function pushTarget(data: unknown): PushTarget | null {
  const value = data as Partial<PushTarget> | null;
  if (!value || typeof value.device !== 'string' || typeof value.agent !== 'number') return null;
  return { device: value.device, agent: value.agent };
}

export type Moved = {
  device: string;
  url: string;
};

export type Delivered = {
  id: string;
  date: number;
  data: unknown;
};

export type Moves = {
  moves: Moved[];
  ids: string[];
};

export function movedTarget(data: unknown): Moved | null {
  const value = data as { device?: unknown; moved?: unknown } | null;
  if (!value || typeof value.device !== 'string' || typeof value.moved !== 'string') return null;
  const url = secureOrigin(value.moved);
  return url ? { device: value.device, url } : null;
}

export function latestMoves(delivered: Delivered[]): Moves {
  const newest = new Map<string, { moved: Moved; date: number }>();
  const ids: string[] = [];
  for (const note of delivered) {
    const moved = movedTarget(note.data);
    if (!moved) continue;
    ids.push(note.id);
    const known = newest.get(moved.device);
    if (!known || note.date >= known.date) newest.set(moved.device, { moved, date: note.date });
  }
  return { moves: [...newest.values()].map(entry => entry.moved), ids };
}

export function shown(paired: boolean, active: boolean, phase: LinkPhase | null): boolean {
  return paired && !(active && phase === 'live');
}
