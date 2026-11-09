import { useSyncExternalStore } from 'react';

export type Listener = () => void;

export class Store<T> {
  private state: T;
  private readonly listeners = new Set<Listener>();

  constructor(initial: T) {
    this.state = initial;
  }

  get = (): T => this.state;

  set(next: T): void {
    if (Object.is(next, this.state)) return;
    this.state = next;
    for (const listener of [...this.listeners]) listener();
  }

  update(change: (state: T) => T): void {
    this.set(change(this.state));
  }

  patch(part: Partial<T>): void {
    this.set({ ...this.state, ...part });
  }

  subscribe = (listener: Listener): (() => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };
}

export function useStore<T>(store: Store<T>): T {
  return useSyncExternalStore(store.subscribe, store.get);
}

export function useSelect<T, S>(store: Store<T>, select: (state: T) => S): S {
  return useSyncExternalStore(store.subscribe, () => select(store.get()));
}
