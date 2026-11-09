import { Easing } from 'react-native';

export const PRESS_MS = 90;
export const FAST_MS = 160;
export const BASE_MS = 240;
export const SLOW_MS = 600;
export const SHEET_MS = 320;
export const REDUCED_MS = 120;
export const EASE_OUT = Easing.bezier(0.22, 1, 0.36, 1);
export const EASE_SPRING = Easing.bezier(0.34, 1.36, 0.64, 1);

export function steps(count: number): (progress: number) => number {
  return progress => Math.min(count - 1, Math.floor(progress * count)) / (count - 1);
}

export function lasting(ms: number, reduced: boolean): number {
  return reduced ? Math.min(ms, REDUCED_MS) : ms;
}
