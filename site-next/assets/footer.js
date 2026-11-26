import { CHIME_DONE, chimeSamples, spitSamples } from './chime.js';

export const SCENE_W = 128;
export const SCENE_H = 96;
export const SCENE_FRAMES = 14;
export const SCENE_KAI = [40, 26, 48];
export const SCENE_IDLE_FRAMES = 4;
export const SCENE_GROW_END = 0.3;
export const SCENE_STARTLE = 0.3;
export const SCENE_IDLE_MS = 900;
export const SCENE_SHIVER_MS = 90;
export const SCENE_STEPS = [[0.4, 6], [0.47, 7], [0.54, 8], [0.62, 9], [0.67, 10], [0.71, 11], [0.75, 12], [0.79, 13]];
export const SCENE_SPIT = 0.67;
export const SCENE_GRAB = 0.54;
export const SCENE_GRAB_LENGTH = 0.08;
export const SCENE_SHAKE = 9;
export const SCENE_INKED = 0.79;
export const SCENE_INFO = 0.82;
export const SCENE_INFO_LENGTH = 0.1;
const KAI_SIZE = 192;
const KAI_SIZE_SMALL = 144;
const KAI_EDGE = 12;
const SMALL_SCREEN = 720;
const HEADER = 88;

const clamp = (value, min = 0, max = 1) => Math.min(Math.max(value, min), max);
const lerp = (a, b, t) => a + (b - a) * t;
const easeOut = (t) => 1 - (1 - t) ** 3;
const easeInOut = (t) => (t < 0.5 ? 4 * t ** 3 : 1 - (-2 * t + 2) ** 3 / 2);
const span = (p, start, length) => clamp((p - start) / length);

export function sceneFrame(p, time) {
  if (p < SCENE_STARTLE) return Math.floor(time / SCENE_IDLE_MS) % SCENE_IDLE_FRAMES;
  if (p < SCENE_STEPS[0][0]) return SCENE_IDLE_FRAMES + (Math.floor(time / SCENE_SHIVER_MS) % 2);
  let frame = SCENE_STEPS[0][1];
  for (const [start, index] of SCENE_STEPS) if (p >= start) frame = index;
  return frame;
}

export function pinned(top, height, viewportHeight, y) {
  return clamp((y - top) / Math.max(1, height - viewportHeight));
}

export function kaiCorner(width, height) {
  const size = width < SMALL_SCREEN ? KAI_SIZE_SMALL : KAI_SIZE;
  return { x: width - size - KAI_EDGE, y: height - size - KAI_EDGE, size };
}

function mountSound() {
  const Context = window.AudioContext || window.webkitAudioContext;
  const sounds = { done: (rate) => chimeSamples(CHIME_DONE, rate), spit: spitSamples };
  const buffers = new Map();
  let audio = null;
  const unlock = () => {
    if (audio || !Context) return;
    audio = new Context();
  };
  for (const type of ['pointerdown', 'keydown', 'touchend']) addEventListener(type, unlock, { once: true, passive: true });
  return (name) => {
    if (!audio || audio.state !== 'running') return;
    if (!buffers.has(name)) {
      const samples = sounds[name](audio.sampleRate);
      const buffer = audio.createBuffer(1, samples.length, audio.sampleRate);
      buffer.copyToChannel(samples, 0);
      buffers.set(name, buffer);
    }
    const source = audio.createBufferSource();
    source.buffer = buffers.get(name);
    source.connect(audio.destination);
    source.start();
  };
}

export function setupFooter({ reduced }) {
  const footer = document.querySelector('[data-ink]');
  if (reduced) return;
  footer.classList.add('is-live');
  const stage = footer.querySelector('[data-ink-stage]');
  const scene = footer.querySelector('[data-scene]');
  const play = mountSound();
  let top = 0;
  let height = 0;
  let width = 0;
  let stageHeight = 0;
  let viewportWidth = 0;
  let viewportHeight = 0;
  let p = 0;
  let shown = -1;
  let visible = false;
  let spat = false;
  let rang = false;
  let pending = false;

  const show = (time) => {
    const frame = sceneFrame(p, time);
    if (frame === shown) return;
    shown = frame;
    scene.style.backgroundPosition = `${(frame / (SCENE_FRAMES - 1)) * 100}% 0`;
  };

  const place = () => {
    pending = false;
    if (!width) return;
    p = pinned(top, height, viewportHeight, scrollY);
    const from = kaiCorner(viewportWidth, viewportHeight);
    const grow = easeInOut(span(p, 0, SCENE_GROW_END));
    const start = from.size / SCENE_KAI[2];
    const end = Math.max(width / SCENE_W, stageHeight / SCENE_H);
    const scale = lerp(start, end, grow);
    const x = lerp(from.x - SCENE_KAI[0] * start, (width - SCENE_W * end) / 2, grow);
    const y = lerp(from.y - SCENE_KAI[1] * start, Math.min(0, (stageHeight - SCENE_H * end) / 2 + HEADER / 2), grow);
    scene.style.width = `${Math.round(SCENE_W * scale)}px`;
    scene.style.height = `${Math.round(SCENE_H * scale)}px`;
    scene.style.transform = `translate3d(${Math.round(x)}px, ${Math.round(y)}px, 0)`;
    scene.style.opacity = p > 0 ? '1' : '0';
    show(performance.now());
    const grab = span(p, SCENE_GRAB, SCENE_GRAB_LENGTH);
    const shake = grab > 0 && grab < 1 ? Math.sin(grab * 46) * (1 - grab) * SCENE_SHAKE : 0;
    stage.style.translate = `${shake}px ${shake * -0.6}px`;
    footer.classList.toggle('is-inked', p >= SCENE_INKED);
    footer.style.setProperty('--info', String(easeOut(span(p, SCENE_INFO, SCENE_INFO_LENGTH))));
    if (p >= SCENE_SPIT && !spat) {
      spat = true;
      play('spit');
    }
    if (p >= SCENE_INFO + SCENE_INFO_LENGTH / 2 && !rang) {
      rang = true;
      play('done');
    }
    if (p < SCENE_STARTLE) {
      spat = false;
      rang = false;
    }
  };

  const request = () => {
    if (pending) return;
    pending = true;
    requestAnimationFrame(place);
  };

  const measure = () => {
    const rect = footer.getBoundingClientRect();
    top = rect.top + scrollY;
    height = rect.height;
    width = stage.clientWidth;
    stageHeight = stage.clientHeight;
    viewportWidth = document.documentElement.clientWidth;
    viewportHeight = innerHeight;
    request();
  };

  const loop = (time) => {
    if (!visible) return;
    show(time);
    requestAnimationFrame(loop);
  };

  new IntersectionObserver(([entry]) => {
    const was = visible;
    visible = entry.isIntersecting;
    if (visible && !was) requestAnimationFrame(loop);
  }).observe(footer);
  addEventListener('scroll', request, { passive: true });
  addEventListener('resize', measure);
  addEventListener('load', measure);
  new ResizeObserver(measure).observe(document.body);
  measure();
}
