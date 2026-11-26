import { storage } from './dom.js';
import { zoomRoute } from './termwire.js';

const ZOOM_KEY = 'octo.uiZoom';
const ZOOM_DEFAULT = 1;
const ZOOM_MIN = 0.8;
const ZOOM_MAX = 1.4;
const ZOOM_STEP = 0.05;

function level() {
  const stored = Number(storage.get(ZOOM_KEY));
  return Number.isFinite(stored) && stored >= ZOOM_MIN && stored <= ZOOM_MAX ? stored : ZOOM_DEFAULT;
}

function paint(scale) {
  document.documentElement.style.setProperty('--ui-zoom', String(scale));
}

export function initUiZoom() {
  paint(level());
}

export function applyUiZoom(route) {
  const current = level();
  const next = route === 'zoom-reset' ? ZOOM_DEFAULT : Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, current + (route === 'zoom-in' ? ZOOM_STEP : -ZOOM_STEP)));
  if (next === current) return;
  storage.set(ZOOM_KEY, String(next));
  paint(next);
}

export { zoomRoute as uiZoomRoute };