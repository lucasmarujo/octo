import { CHIME_DONE, chimeSamples, spitSamples } from './chime.js';
import { SPINNER } from './tui.js';

const REDUCED_MOTION = matchMedia('(prefers-reduced-motion: reduce)').matches;
const FINE_POINTER = matchMedia('(hover: hover) and (pointer: fine)').matches;
const TICK_MS = 100;
const KEY_MS = 1_100;
const KEY_GAP_MS = 600;
const TOUR_KEYS = [['enter'], [], ['alt+n', 'ctrl+p'], ['ctrl+t'], ['ctrl+r']];
const PIXEL_CELL = 22;
const TRAIL_FADE_MS = 650;
const TRAIL_STEPS = 4;
const TRAIL_SPREAD = 0.35;
const TRAIL_MAX_STEPS = 64;
const TRAIL_NEIGHBORS = [[-1, 0], [1, 0], [0, -1], [0, 1], [-1, -1], [1, -1], [-1, 1], [1, 1]];
const DIVER_MOODS = { topo: 'idle', problema: 'needs', recursos: 'working', agentes: 'working', 'como-funciona': 'sleep', teclado: 'idle', seguranca: 'idle', instalar: 'done', video: 'idle' };
const DIVER_DIZZY_SPEED = 6;
const DIVER_DIZZY_MS = 900;
const DIVER_TILT = 14;
const DIVER_EDGE = 12;
const DIVER_HOP = 150;
const DIVER_JUMP_MS = 720;
const SCENE_W = 128;
const SCENE_H = 96;
const SCENE_FRAMES = 14;
const SCENE_KAI = [40, 26, 48];
const SCENE_IDLE_FRAMES = 4;
const SCENE_GROW_END = 0.3;
const SCENE_STARTLE = 0.3;
const SCENE_IDLE_MS = 900;
const SCENE_SHIVER_MS = 90;
const SCENE_STEPS = [[0.4, 6], [0.47, 7], [0.54, 8], [0.62, 9], [0.67, 10], [0.71, 11], [0.75, 12], [0.79, 13]];
const SCENE_SPIT = 0.67;
const SCENE_GRAB = 0.54;
const SCENE_GRAB_LENGTH = 0.08;
const SCENE_SHAKE = 9;
const SCENE_INKED = 0.79;
const SCENE_INFO = 0.82;
const SCENE_INFO_LENGTH = 0.1;
const SCRAMBLE_GLYPHS = '0123456789abcdef#%&*+=';
const CHAOS_SCATTER = [[0.02, 0.06, -7], [0.62, 0, 5], [0.3, 0.34, -3], [0.76, 0.4, 8], [0.05, 0.66, 4], [0.47, 0.7, -6], [0.84, 0.8, -9], [0.2, 0.16, 3]];

let frame = 0;
const tracked = [];
const scrollers = [];
const layouts = [];
const viewport = { w: innerWidth, h: innerHeight, doc: 1 };
let framePending = false;

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const clamp = (value, min = 0, max = 1) => Math.min(Math.max(value, min), max);
const lerp = (a, b, t) => a + (b - a) * t;
const easeOut = (t) => 1 - (1 - t) ** 3;
const easeInOut = (t) => (t < 0.5 ? 4 * t ** 3 : 1 - (-2 * t + 2) ** 3 / 2);
const span = (p, start, length) => clamp((p - start) / length);

function el(tag, className, text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}

function topHeight() {
  return parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--top-h')) || 64;
}

function setText(node, text) {
  if (node.textContent !== text) node.textContent = text;
}

function whenVisible(node, onChange, rootMargin = '120px 0px') {
  if (!('IntersectionObserver' in window)) {
    onChange(true);
    return;
  }
  new IntersectionObserver(([entry]) => onChange(entry.isIntersecting), { rootMargin }).observe(node);
}

function track(node) {
  const item = { node, top: 0, height: 0 };
  tracked.push(item);
  return item;
}

function measure() {
  viewport.w = document.documentElement.clientWidth;
  viewport.h = innerHeight;
  viewport.doc = Math.max(1, document.documentElement.scrollHeight - innerHeight);
  for (const item of tracked) {
    const rect = item.node.getBoundingClientRect();
    item.top = rect.top + scrollY;
    item.height = rect.height;
  }
  layouts.forEach((layout) => layout());
  requestFrame();
}

function enter(item, y, start = 1, end = 0.3) {
  return clamp((y + viewport.h * start - item.top) / (viewport.h * (start - end)));
}

function pinned(item, y, offset) {
  return clamp((y + offset - item.top) / Math.max(1, item.height - viewport.h + offset));
}

function requestFrame() {
  if (framePending) return;
  framePending = true;
  requestAnimationFrame(() => {
    framePending = false;
    const y = scrollY;
    scrollers.forEach((scroller) => scroller(y));
  });
}

function startScroll() {
  addEventListener('scroll', requestFrame, { passive: true });
  addEventListener('resize', measure);
  addEventListener('load', measure);
  document.fonts.ready.then(measure);
  if ('ResizeObserver' in window) new ResizeObserver(() => measure()).observe(document.body);
  measure();
}

function startClock() {
  setInterval(() => {
    if (document.hidden) return;
    if (!REDUCED_MOTION) frame += 1;
    for (const node of document.querySelectorAll('[data-spin]')) setText(node, SPINNER[frame % SPINNER.length]);
  }, TICK_MS);
}

function showKey(keycap, key) {
  setText(keycap, key);
  keycap.classList.add('is-shown');
  setTimeout(() => keycap.classList.remove('is-shown'), KEY_MS);
}

function splitWords(node) {
  let index = 0;
  const walk = (parent) => {
    for (const child of [...parent.childNodes]) {
      if (child.nodeType === Node.ELEMENT_NODE) {
        walk(child);
        continue;
      }
      if (child.nodeType !== Node.TEXT_NODE) continue;
      const fragment = document.createDocumentFragment();
      for (const part of child.textContent.split(/(\s+)/)) {
        if (!part) continue;
        if (/^\s+$/.test(part)) {
          fragment.append(part);
          continue;
        }
        const inner = el('span', '', part);
        inner.style.setProperty('--d', `${index * 0.06}s`);
        index += 1;
        const outer = el('span', 'w');
        outer.append(inner);
        fragment.append(outer);
      }
      child.replaceWith(fragment);
    }
  };
  walk(node);
  node.classList.add('is-split');
}

function scramble(node, delay) {
  const text = node.textContent;
  const duration = 1_000;
  let start = 0;
  const step = (time) => {
    start ||= time + delay;
    const progress = (time - start) / duration;
    if (progress < 0) {
      requestAnimationFrame(step);
      return;
    }
    let out = '';
    [...text].forEach((char, i) => {
      const settle = 0.3 + (i / text.length) * 0.6;
      out += char === ' ' || char === '.' || progress >= settle ? char : SCRAMBLE_GLYPHS[Math.floor(Math.random() * SCRAMBLE_GLYPHS.length)];
    });
    node.textContent = out;
    if (progress < 1) requestAnimationFrame(step);
    else node.textContent = text;
  };
  requestAnimationFrame(step);
}

function mountReveal() {
  const splits = document.querySelectorAll('[data-split]');
  if (!REDUCED_MOTION) splits.forEach(splitWords);
  const items = document.querySelectorAll('[data-reveal], [data-split]');
  const show = (node) => {
    node.classList.add('in');
    if (!REDUCED_MOTION) node.querySelectorAll('[data-scramble]').forEach((value, i) => scramble(value, i * 140));
  };
  const loaders = document.querySelectorAll('[data-split="load"]');
  requestAnimationFrame(() => requestAnimationFrame(() => loaders.forEach(show)));
  if (!('IntersectionObserver' in window)) {
    items.forEach(show);
    return;
  }
  const observer = new IntersectionObserver((entries) => {
    for (const entry of entries) {
      if (!entry.isIntersecting) continue;
      show(entry.target);
      observer.unobserve(entry.target);
    }
  }, { rootMargin: '0px 0px -10% 0px' });
  items.forEach((item) => {
    if (item.dataset.split !== 'load') observer.observe(item);
  });
}

function mountHeader(top) {
  const bar = top.querySelector('[data-progress]');
  const runner = top.querySelector('[data-runner]');
  let idle = 0;
  let runnerWidth = 0;
  layouts.push(() => {
    runnerWidth = runner.offsetWidth;
  });
  scrollers.push((y) => {
    top.classList.toggle('is-scrolled', y > 8);
    const progress = clamp(y / viewport.doc);
    bar.style.transform = `scaleX(${progress})`;
    runner.style.transform = `translate3d(${progress * (viewport.w - runnerWidth)}px, 0, 0)`;
    if (REDUCED_MOTION) return;
    runner.classList.add('is-walking');
    clearTimeout(idle);
    idle = setTimeout(() => runner.classList.remove('is-walking'), 160);
  });
}

function mountHero(hero) {
  if (REDUCED_MOTION) return;
  const [first, second] = hero.querySelectorAll('.display .line');
  const copy = hero.querySelector('.hero-copy');
  const mascot = hero.querySelector('.hero-mascot');
  const stage = hero.querySelector('[data-tilt]');
  const screen = stage.querySelector('[data-screen]');
  const heroItem = track(hero);
  const stageItem = track(stage);
  scrollers.push((y) => {
    const p = clamp((y - heroItem.top) / (viewport.h * 0.9));
    first.style.translate = `${-p * 14}vw 0`;
    second.style.translate = `${p * 10}vw 0`;
    copy.style.translate = `0 ${-p * 60}px`;
    mascot.style.translate = `0 ${-p * 140}px`;
    const t = easeOut(enter(stageItem, y, 1, 0.28));
    screen.style.transform = `rotateX(${(1 - t) * 26}deg) scale(${0.9 + t * 0.1}) translate3d(0, ${(1 - t) * 40}px, 0)`;
  });
}

function mountKai(kai) {
  let cheer = 0;
  kai.addEventListener('click', () => {
    kai.classList.remove('cheer');
    void kai.offsetWidth;
    kai.classList.add('cheer');
    clearTimeout(cheer);
    cheer = setTimeout(() => kai.classList.remove('cheer'), 1300);
  });
}

function mountMarquee(rows) {
  if (REDUCED_MOTION) return;
  rows.forEach((row) => {
    const trackNode = row.querySelector('.marquee-track');
    const clone = trackNode.cloneNode(true);
    clone.setAttribute('aria-hidden', 'true');
    row.append(clone);
    const direction = Number(row.dataset.marquee);
    let width = 1;
    let visible = false;
    let running = false;
    let drift = 0;
    let last = 0;
    layouts.push(() => {
      width = trackNode.offsetWidth || 1;
    });
    const loop = (time) => {
      if (!visible) {
        running = false;
        return;
      }
      drift += Math.min(time - (last || time), 64) * 0.045;
      last = time;
      const offset = (drift + scrollY * 0.55) % width;
      const x = direction > 0 ? -offset : offset - width;
      trackNode.style.transform = `translate3d(${x}px, 0, 0)`;
      clone.style.transform = `translate3d(${x}px, 0, 0)`;
      requestAnimationFrame(loop);
    };
    whenVisible(row, (isVisible) => {
      visible = isVisible;
      if (visible && !running) {
        running = true;
        last = 0;
        requestAnimationFrame(loop);
      }
    }, '0px');
  });
}

function mountChaos(section) {
  if (REDUCED_MOTION) return;
  section.classList.add('is-live');
  const field = section.querySelector('[data-chaos-field]');
  const wins = [...section.querySelectorAll('.win')];
  const rows = [...section.querySelectorAll('.chaos-row')];
  const question = section.querySelector('.chaos-q');
  const answer = section.querySelector('.chaos-a');
  const before = section.querySelector('.chaos-before');
  const after = section.querySelector('.chaos-after');
  const item = track(section);
  let poses = [];
  layouts.push(() => {
    const box = field.getBoundingClientRect();
    poses = wins.map((win, i) => {
      const row = rows[i].getBoundingClientRect();
      const width = win.offsetWidth;
      const height = win.offsetHeight;
      const scale = clamp((row.height * 1.4) / height, 0.18, 1);
      const [fx, fy, rotate] = CHAOS_SCATTER[i];
      return {
        from: [fx * (box.width - width), fy * (box.height - height), rotate],
        to: [row.left - box.left, row.top - box.top + row.height / 2 - (height * scale) / 2, 0],
        scale,
      };
    });
  });
  let offset = 64;
  layouts.push(() => {
    offset = topHeight();
  });
  scrollers.push((y) => {
    if (!poses.length) return;
    const p = pinned(item, y, offset);
    wins.forEach((win, i) => {
      const pose = poses[i];
      const appear = easeOut(span(p, 0.01 + i * 0.03, 0.1));
      const converge = span(p, 0.46 + i * 0.03, 0.2);
      const e = easeInOut(converge);
      const sway = (1 - e) * (p * 90 - 30) * (i % 2 ? 1 : -1);
      const x = lerp(pose.from[0], pose.to[0], e);
      const yy = lerp(pose.from[1] + sway, pose.to[1], e);
      const rotate = lerp(pose.from[2], 0, e);
      const scale = lerp(0.86 + 0.14 * appear, pose.scale, e);
      win.style.transform = `translate3d(${x}px, ${yy}px, 0) rotate(${rotate}deg) scale(${scale})`;
      win.style.opacity = String(appear * (1 - span(converge, 0.62, 0.38)));
      win.classList.toggle('is-ringing', converge === 0 && p > 0.2 + (i % 3) * 0.04 && (win.classList.contains('is-ask') || win.classList.contains('is-done')));
    });
    section.style.setProperty('--built', String(span(p, 0.48, 0.14)));
    const swap = span(p, 0.7, 0.08);
    question.style.opacity = String(1 - swap);
    question.style.translate = `0 ${-swap * 24}px`;
    answer.style.opacity = String(span(p, 0.75, 0.08));
    answer.style.translate = `0 ${(1 - span(p, 0.75, 0.08)) * 24}px`;
    before.style.opacity = String(1 - span(p, 0.6, 0.08));
    after.style.opacity = String(span(p, 0.78, 0.08));
  });
}

function mountTour(root) {
  root.classList.add('is-live');
  const shots = [...root.querySelectorAll('[data-shot]')];
  const keycap = root.querySelector('[data-tour-key]');
  const steps = [...root.querySelectorAll('.tour-step')];
  const dots = [...root.querySelectorAll('.tour-dots li')];
  let token = 0;
  let active = -1;

  async function press(mine, keys) {
    for (const key of keys) {
      await sleep(KEY_GAP_MS);
      if (mine !== token) return;
      showKey(keycap, key);
      await sleep(KEY_MS);
    }
  }

  function activate(index) {
    if (index === active) return;
    active = index;
    token += 1;
    steps.forEach((s, i) => s.classList.toggle('is-active', i === index));
    dots.forEach((d, i) => d.classList.toggle('is-on', i <= index));
    shots.forEach((shot, i) => shot.classList.toggle('is-on', i === index));
    press(token, TOUR_KEYS[index]);
  }

  whenVisible(root, (isVisible) => {
    if (isVisible && active < 0) activate(0);
  }, '0px');
  if (!('IntersectionObserver' in window)) return;
  const observer = new IntersectionObserver((entries) => {
    for (const entry of entries) {
      if (entry.isIntersecting) activate(steps.indexOf(entry.target));
    }
  }, { rootMargin: '-48% 0px -48% 0px' });
  steps.forEach((s) => observer.observe(s));
}

function mountUsage(root) {
  root.querySelectorAll('[data-pct]').forEach((fill) => {
    fill.style.width = `${fill.dataset.pct}%`;
  });
}

function mountPlayer(root) {
  const video = root.querySelector('[data-video]');
  root.querySelector('[data-play]').addEventListener('click', () => {
    root.classList.add('is-playing');
    video.controls = true;
    video.focus();
    video.play().catch(() => {});
  });
  if (REDUCED_MOTION) return;
  const item = track(root);
  scrollers.push((y) => {
    const p = easeOut(enter(item, y, 1, 0.32));
    root.style.clipPath = `inset(${(1 - p) * 9}% ${(1 - p) * 16}% ${(1 - p) * 9}% ${(1 - p) * 16}%)`;
    video.style.transform = `scale(${1.2 - p * 0.2})`;
  });
}

function mountArch(root) {
  const label = root.querySelector('[data-arch-state]');
  const original = label.textContent;
  let visible = false;
  whenVisible(root, (isVisible) => {
    visible = isVisible;
  });
  (async () => {
    for (;;) {
      await sleep(3_200);
      if (!visible || document.hidden) continue;
      root.classList.add('is-closed');
      setText(label, 'closed · agents keep running');
      await sleep(2_400);
      root.classList.remove('is-closed');
      setText(label, 'reconnected to all');
      await sleep(1_600);
      setText(label, original);
    }
  })();
}

function mountKeys(list) {
  const items = new Map([...list.querySelectorAll('[data-key]')].map((item) => [item.dataset.key, item]));
  const press = (name) => {
    const item = items.get(name);
    if (!item) return;
    item.classList.add('is-pressed');
    setTimeout(() => item.classList.remove('is-pressed'), 260);
  };
  addEventListener('keydown', (event) => {
    const key = event.key.toLowerCase();
    if (key === 'enter' || key === 'tab' || key === '/' || key === '!') press(key);
    else if (event.ctrlKey && ['l', 't', ']', 'r'].includes(key)) press(`ctrl+${key}`);
    else if (event.altKey && (key === 'n' || event.code === 'KeyN')) press('alt+n');
  });
  if (REDUCED_MOTION) return;
  let played = false;
  whenVisible(list, async (isVisible) => {
    if (!isVisible || played) return;
    played = true;
    await sleep(500);
    for (const name of items.keys()) {
      press(name);
      await sleep(110);
    }
  }, '0px 0px -20% 0px');
}

function mountFinale(section) {
  const canvas = section.querySelector('[data-pixels]');
  const context = canvas.getContext('2d');
  const mascot = section.querySelector('[data-walk]');
  const item = track(section);
  const cell = PIXEL_CELL;
  const pink = getComputedStyle(document.documentElement).getPropertyValue('--pink').trim();
  let columns = 0;
  let rowsCount = 0;
  let noise = [];
  let lastDrawn = -1;
  let idle = 0;
  layouts.push(() => {
    const ratio = Math.min(devicePixelRatio || 1, 2);
    canvas.width = Math.round(canvas.clientWidth * ratio);
    canvas.height = Math.round(canvas.clientHeight * ratio);
    context.setTransform(ratio, 0, 0, ratio, 0, 0);
    columns = Math.ceil(canvas.clientWidth / cell);
    rowsCount = Math.ceil(canvas.clientHeight / cell);
    noise = Array.from({ length: columns * rowsCount }, (_, i) => {
      const s = Math.sin(i * 12.9898 + 78.233) * 43758.5453;
      return s - Math.floor(s);
    });
    lastDrawn = -1;
  });
  const draw = (p) => {
    if (Math.abs(p - lastDrawn) < 0.004) return;
    lastDrawn = p;
    const height = canvas.clientHeight;
    context.clearRect(0, 0, canvas.clientWidth, height);
    context.fillStyle = pink;
    for (let r = 0; r < rowsCount; r += 1) {
      for (let c = 0; c < columns; c += 1) {
        const threshold = (r / rowsCount) * 0.8 + noise[r * columns + c] * 0.35;
        if (p * 0.95 > threshold) context.fillRect(c * cell, height - (r + 1) * cell, cell, cell);
      }
    }
  };
  scrollers.push((y) => {
    if (!columns) return;
    draw(REDUCED_MOTION ? 1 : easeOut(enter(item, y, 1.15, 0.55)));
    if (REDUCED_MOTION) return;
    const walk = easeOut(enter(item, y, 0.95, 0.35));
    mascot.style.transform = `translate3d(${(1 - walk) * -45}vw, 0, 0)`;
    if (walk > 0 && walk < 1) {
      mascot.classList.add('is-walking');
      clearTimeout(idle);
      idle = setTimeout(() => mascot.classList.remove('is-walking'), 160);
    }
  });
}

function mountTrail(section) {
  if (REDUCED_MOTION || !FINE_POINTER) return;
  const canvas = section.querySelector('[data-trail]');
  const pixels = section.querySelector('[data-pixels]');
  const context = canvas.getContext('2d');
  const ink = getComputedStyle(document.documentElement).getPropertyValue('--ink').trim();
  const cells = new Map();
  let columns = 0;
  let last = null;
  let running = false;
  layouts.push(() => {
    const above = Math.ceil(pixels.offsetHeight / PIXEL_CELL) * PIXEL_CELL;
    canvas.style.top = `${-above}px`;
    canvas.style.height = `${above + section.offsetHeight}px`;
    const ratio = Math.min(devicePixelRatio || 1, 2);
    canvas.width = Math.round(canvas.clientWidth * ratio);
    canvas.height = Math.round(canvas.clientHeight * ratio);
    context.setTransform(ratio, 0, 0, ratio, 0, 0);
    columns = Math.ceil(canvas.clientWidth / PIXEL_CELL);
    cells.clear();
  });
  const stamp = (c, r, now) => {
    if (c >= 0 && r >= 0 && c < columns) cells.set(r * columns + c, now);
  };
  const paint = (x, y, now) => {
    const c = Math.floor(x / PIXEL_CELL);
    const r = Math.floor(y / PIXEL_CELL);
    stamp(c, r, now);
    for (const [dc, dr] of TRAIL_NEIGHBORS) {
      if (Math.random() < TRAIL_SPREAD) stamp(c + dc, r + dr, now);
    }
  };
  const draw = (now) => {
    context.clearRect(0, 0, canvas.clientWidth, canvas.clientHeight);
    context.fillStyle = ink;
    for (const [key, born] of cells) {
      const life = 1 - (now - born) / TRAIL_FADE_MS;
      if (life <= 0) {
        cells.delete(key);
        continue;
      }
      context.globalAlpha = Math.ceil(life * TRAIL_STEPS) / TRAIL_STEPS;
      context.fillRect((key % columns) * PIXEL_CELL, Math.floor(key / columns) * PIXEL_CELL, PIXEL_CELL, PIXEL_CELL);
    }
    context.globalAlpha = 1;
    if (cells.size) requestAnimationFrame(draw);
    else running = false;
  };
  addEventListener('pointermove', (event) => {
    if (!columns) return;
    const rect = canvas.getBoundingClientRect();
    const x = event.clientX - rect.left;
    const y = event.clientY - rect.top;
    if (x < 0 || y < 0 || x > rect.width || y > rect.height) {
      last = null;
      return;
    }
    const from = last || { x, y };
    const steps = Math.min(TRAIL_MAX_STEPS, Math.max(1, Math.ceil(Math.hypot(x - from.x, y - from.y) / (PIXEL_CELL / 2))));
    const now = performance.now();
    for (let i = 1; i <= steps; i += 1) paint(lerp(from.x, x, i / steps), lerp(from.y, y, i / steps), now);
    last = { x, y };
    if (running) return;
    running = true;
    requestAnimationFrame(draw);
  });
  document.documentElement.addEventListener('pointerleave', () => {
    last = null;
  });
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
  ['pointerdown', 'keydown', 'touchend'].forEach((type) => addEventListener(type, unlock, { once: true, passive: true }));
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

function mountDiver(diver, footer) {
  if (REDUCED_MOTION) return null;
  const sprite = diver.querySelector('.kai-sprite');
  const footerItem = track(footer);
  const sections = Object.keys(DIVER_MOODS);
  let size = 192;
  let top = 64;
  let side = 1;
  let x = 0;
  let y = 0;
  let jump = null;
  let mood = 'idle';
  let dizzy = 0;
  let lastY = scrollY;
  let lastTime = performance.now();
  let settle = 0;
  const sideX = (which) => (which < 0 ? DIVER_EDGE : viewport.w - size - DIVER_EDGE);
  const show = () => {
    const next = dizzy ? 'lost' : mood;
    if (diver.dataset.mood !== next) diver.dataset.mood = next;
  };
  const place = (now) => {
    let hop = 0;
    let lean = 0;
    if (jump) {
      const t = clamp((now - jump.start) / DIVER_JUMP_MS);
      x = lerp(jump.from, sideX(side), easeInOut(t));
      hop = Math.sin(Math.PI * t) * DIVER_HOP;
      lean = Math.sin(Math.PI * t) * -side * DIVER_TILT;
      sprite.style.scale = t < 0.12 ? '1.08 0.9' : t > 0.9 ? '1.06 0.92' : '0.94 1.06';
      if (t >= 1) {
        jump = null;
        sprite.style.scale = '1 1';
      }
    } else {
      x = sideX(side);
    }
    diver.style.transform = `translate3d(${x}px, ${y - hop}px, 0) rotate(${lean}deg)`;
  };
  const leap = () => {
    if (!jump) return;
    place(performance.now());
    if (jump) requestAnimationFrame(leap);
  };
  layouts.push(() => {
    size = diver.offsetWidth;
    top = topHeight() + DIVER_EDGE;
    place(performance.now());
  });
  if ('IntersectionObserver' in window) {
    const observer = new IntersectionObserver((entries) => {
      for (const entry of entries) {
        if (!entry.isIntersecting) continue;
        mood = DIVER_MOODS[entry.target.id];
        show();
        const next = sections.indexOf(entry.target.id) % 2 ? -1 : 1;
        if (next === side) continue;
        side = next;
        const running = Boolean(jump);
        jump = { from: x, start: performance.now() };
        if (!running) requestAnimationFrame(leap);
      }
    }, { rootMargin: '-50% 0px -50% 0px' });
    sections.forEach((id) => {
      const section = document.getElementById(id);
      if (section) observer.observe(section);
    });
  }
  scrollers.push((scroll) => {
    const now = performance.now();
    const delta = scroll - lastY;
    const speed = Math.abs(delta) / Math.max(1, now - lastTime);
    lastY = scroll;
    lastTime = now;
    if (speed > DIVER_DIZZY_SPEED) {
      clearTimeout(dizzy);
      dizzy = setTimeout(() => {
        dizzy = 0;
        show();
      }, DIVER_DIZZY_MS);
    }
    show();
    const progress = clamp(scroll / Math.max(1, footerItem.top - viewport.h));
    y = top + progress * (viewport.h - top - size - DIVER_EDGE);
    place(now);
    diver.classList.toggle('is-on', scroll > viewport.h * 0.6 && scroll < footerItem.top);
    sprite.style.rotate = `${clamp(delta * 0.4, -DIVER_TILT, DIVER_TILT)}deg`;
    clearTimeout(settle);
    settle = setTimeout(() => {
      sprite.style.rotate = '0deg';
    }, 140);
  });
  return { end: () => ({ x, y: viewport.h - size - DIVER_EDGE, size }) };
}

function sceneFrame(p, time) {
  if (p < SCENE_STARTLE) return Math.floor(time / SCENE_IDLE_MS) % SCENE_IDLE_FRAMES;
  if (p < SCENE_STEPS[0][0]) return SCENE_IDLE_FRAMES + (Math.floor(time / SCENE_SHIVER_MS) % 2);
  let frame = SCENE_STEPS[0][1];
  for (const [start, index] of SCENE_STEPS) if (p >= start) frame = index;
  return frame;
}

function mountInk(footer, diver, play) {
  if (!diver) return;
  footer.classList.add('is-live');
  const stage = footer.querySelector('[data-ink-stage]');
  const scene = footer.querySelector('[data-scene]');
  const item = track(footer);
  let width = 0;
  let height = 0;
  let header = 64;
  let p = 0;
  let shown = -1;
  let visible = false;
  let spat = false;
  let rang = false;

  layouts.push(() => {
    width = stage.clientWidth;
    height = stage.clientHeight;
    header = topHeight();
  });

  const show = (time) => {
    const frame = sceneFrame(p, time);
    if (frame === shown) return;
    shown = frame;
    scene.style.backgroundPosition = `${(frame / (SCENE_FRAMES - 1)) * 100}% 0`;
  };

  const loop = (time) => {
    if (!visible) return;
    show(time);
    requestAnimationFrame(loop);
  };
  whenVisible(footer, (isVisible) => {
    const wasVisible = visible;
    visible = isVisible;
    if (visible && !wasVisible) requestAnimationFrame(loop);
  }, '0px');

  scrollers.push((y) => {
    if (!width) return;
    p = pinned(item, y, 0);
    const from = diver.end();
    const grow = easeInOut(span(p, 0, SCENE_GROW_END));
    const start = from.size / SCENE_KAI[2];
    const end = Math.max(width / SCENE_W, height / SCENE_H);
    const scale = lerp(start, end, grow);
    const x = lerp(from.x - SCENE_KAI[0] * start, (width - SCENE_W * end) / 2, grow);
    const top = lerp(from.y - SCENE_KAI[1] * start, Math.min(0, (height - SCENE_H * end) / 2 + header / 2), grow);
    scene.style.width = `${Math.round(SCENE_W * scale)}px`;
    scene.style.height = `${Math.round(SCENE_H * scale)}px`;
    scene.style.transform = `translate3d(${Math.round(x)}px, ${Math.round(top)}px, 0)`;
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
  });
}

mountReveal();
mountHeader(document.querySelector('[data-top]'));
mountHero(document.querySelector('.hero'));
mountKai(document.querySelector('[data-kai]'));
mountMarquee(document.querySelectorAll('[data-marquee]'));
mountChaos(document.querySelector('[data-chaos]'));
mountTour(document.querySelector('[data-tour]'));
mountUsage(document.querySelector('.usage-visual'));
mountPlayer(document.querySelector('[data-player]'));
mountArch(document.querySelector('[data-arch]'));
mountKeys(document.querySelector('[data-keys]'));
mountFinale(document.querySelector('[data-finale]'));
mountTrail(document.querySelector('[data-finale]'));
mountInk(document.querySelector('[data-ink]'), mountDiver(document.querySelector('[data-diver]'), document.querySelector('[data-ink]')), mountSound());
startClock();
startScroll();
