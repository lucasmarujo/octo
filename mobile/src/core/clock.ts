import { useSyncExternalStore } from 'react';

import type { Listener } from './store';

const FRAME_MS = 100;
const SECOND_MS = 1000;

class Ticker {
  private count = 0;
  private timer: ReturnType<typeof setInterval> | null = null;
  private readonly listeners = new Set<Listener>();

  constructor(private readonly every: number) {}

  get = (): number => this.count;

  subscribe = (listener: Listener): (() => void) => {
    this.listeners.add(listener);
    if (!this.timer) this.timer = setInterval(this.tick, this.every);
    return () => {
      this.listeners.delete(listener);
      if (!this.listeners.size && this.timer) {
        clearInterval(this.timer);
        this.timer = null;
      }
    };
  };

  private tick = (): void => {
    this.count += 1;
    for (const listener of [...this.listeners]) listener();
  };
}

const frames = new Ticker(FRAME_MS);
const seconds = new Ticker(SECOND_MS);

export function useFrame(): number {
  return useSyncExternalStore(frames.subscribe, frames.get);
}

export function useSecond(): number {
  return useSyncExternalStore(seconds.subscribe, seconds.get);
}
