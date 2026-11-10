import type { LinkPhase } from './link';
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

export function shown(paired: boolean, active: boolean, phase: LinkPhase | null): boolean {
  return paired && !(active && phase === 'live');
}
