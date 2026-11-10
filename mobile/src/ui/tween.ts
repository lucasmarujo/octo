import { useEffect, useRef, useState } from 'react';

const STEP_MS = 16;

export function useTween(target: number, ms: number, ease: (progress: number) => number, from?: number): number {
  const [value, setValue] = useState(from ?? target);
  const current = useRef(from ?? target);

  useEffect(() => {
    const start = current.current;
    if (ms <= 0 || start === target) {
      current.current = target;
      setValue(target);
      return undefined;
    }
    const began = Date.now();
    const timer = setInterval(() => {
      const progress = Math.min(1, (Date.now() - began) / ms);
      const next = start + (target - start) * ease(progress);
      current.current = next;
      setValue(next);
      if (progress >= 1) clearInterval(timer);
    }, STEP_MS);
    return () => clearInterval(timer);
  }, [target, ms, ease]);

  return value;
}
